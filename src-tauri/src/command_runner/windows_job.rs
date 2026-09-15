//! Windows Job Object session lifecycle (F003).
//!
//! Spawn: CreateJobObject → CreateProcessW(CREATE_SUSPENDED) → AssignProcessToJobObject → ResumeThread.
//! Stop: TerminateJobObject. Flag: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE only.
//! Handle inheritance: STARTUPINFOEX + PROC_THREAD_ATTRIBUTE_HANDLE_LIST only exposes session stdio pipes.

use std::ffi::c_void;
use std::fs::File;
use std::mem::{size_of, size_of_val};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::Path;
#[cfg(test)]
use std::sync::atomic::AtomicBool;

#[cfg(test)]
use std::sync::atomic::Ordering;

use windows::Win32::Foundation::{
    CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, STILL_ACTIVE, WAIT_OBJECT_0,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JobObjectExtendedLimitInformation,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    SetInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess, GetProcessId,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcess, ResumeThread,
    TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject, PROCESS_INFORMATION,
    PROCESS_QUERY_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES,
    STARTUPINFOEXW, STARTUPINFOW, CREATE_NO_WINDOW, CREATE_SUSPENDED,
    EXTENDED_STARTUPINFO_PRESENT, INFINITE,
};

#[cfg(test)]
static TEST_FORCE_JOB_TERMINATE_FAIL: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
pub fn test_force_job_terminate_fail() -> bool {
    TEST_FORCE_JOB_TERMINATE_FAIL.load(Ordering::SeqCst)
}

#[cfg(not(test))]
pub fn test_force_job_terminate_fail() -> bool {
    false
}

pub struct WinHandle(HANDLE);

// Windows kernel handles are safe to send between threads when owned exclusively.
unsafe impl Send for WinHandle {}
unsafe impl Sync for WinHandle {}

impl WinHandle {
    fn new(handle: HANDLE) -> Self {
        Self(handle)
    }

    fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for WinHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// Owns the backing memory required by a Win32 process-thread attribute list.
/// The list only lives through CreateProcessW and is deleted before returning.
struct ProcThreadAttributeList {
    _storage: Vec<u8>,
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
}

impl ProcThreadAttributeList {
    fn for_handle_list(handles: &[HANDLE]) -> Result<Self, String> {
        debug_assert!(!handles.is_empty());

        let mut bytes = 0usize;
        // The sizing call is documented to fail while returning the required byte count.
        let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut bytes) };
        if bytes == 0 {
            return Err("InitializeProcThreadAttributeList(size): 未返回缓冲区大小".to_string());
        }

        let mut storage = vec![0u8; bytes];
        let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr() as *mut c_void);
        unsafe {
            InitializeProcThreadAttributeList(Some(list), 1, None, &mut bytes)
                .map_err(|e| format!("InitializeProcThreadAttributeList: {}", e))?;
        }

        let attributes = Self {
            _storage: storage,
            list,
        };
        unsafe {
            UpdateProcThreadAttribute(
                attributes.list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                Some(handles.as_ptr() as *const c_void),
                size_of_val(handles),
                None,
                None,
            )
        }
        .map_err(|e| format!("UpdateProcThreadAttribute(HANDLE_LIST): {}", e))?;

        Ok(attributes)
    }
}

impl Drop for ProcThreadAttributeList {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.list);
        }
    }
}

pub struct SessionJob(WinHandle);

impl SessionJob {
    pub fn new() -> Result<Self, String> {
        let job = unsafe { CreateJobObjectW(None, None) }
            .map_err(|e| format!("CreateJobObjectW: {}", e))?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
        .map_err(|e| format!("SetInformationJobObject: {}", e))?;
        Ok(Self(WinHandle::new(job)))
    }

    pub fn assign_process(&self, process: HANDLE) -> Result<(), String> {
        unsafe { AssignProcessToJobObject(self.0 .0, process) }
            .map_err(|e| format!("AssignProcessToJobObject: {}", e))
    }

    pub fn terminate(&self) -> Result<(), String> {
        if test_force_job_terminate_fail() {
            return Err("LAUNCH_STOP_FAILED:job terminate forced fail (test)".to_string());
        }
        unsafe { TerminateJobObject(self.0 .0, 1) }
            .map_err(|e| format!("TerminateJobObject: {}", e))
    }

    pub fn raw(&self) -> HANDLE {
        self.0 .0
    }
}

pub struct OwnedProcess(WinHandle);

impl OwnedProcess {
    /// Returns `(exit_code, wait_failed)`.
    pub fn wait(&self) -> (Option<i32>, bool) {
        unsafe {
            if WaitForSingleObject(self.0 .0, INFINITE) != WAIT_OBJECT_0 {
                return (None, true);
            }
            let mut code = 0u32;
            if GetExitCodeProcess(self.0 .0, &mut code).is_err() {
                return (None, true);
            }
            if code == STILL_ACTIVE.0 as u32 {
                return (None, true);
            }
            (Some(code as i32), false)
        }
    }

    pub fn raw(&self) -> HANDLE {
        self.0 .0
    }
}

struct PipePair {
    read: WinHandle,
    write: WinHandle,
}

impl PipePair {
    fn new() -> Result<Self, String> {
        let mut read = HANDLE::default();
        let mut write = HANDLE::default();
        unsafe {
            CreatePipe(&mut read, &mut write, None, 0)
                .map_err(|e| format!("CreatePipe: {}", e))?;
            SetHandleInformation(read, HANDLE_FLAG_INHERIT.0, Default::default())
                .map_err(|e| format!("SetHandleInformation(read): {}", e))?;
            SetHandleInformation(write, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT)
                .map_err(|e| format!("SetHandleInformation(write): {}", e))?;
        }
        Ok(Self {
            read: WinHandle::new(read),
            write: WinHandle::new(write),
        })
    }

    fn write_handle(&self) -> HANDLE {
        self.write.raw()
    }

    fn close_write(&mut self) {
        self.write = WinHandle::new(HANDLE::default());
    }

    fn into_read_file(mut self) -> File {
        self.close_write();
        let handle = self.read.0;
        std::mem::forget(self.read);
        unsafe { File::from_raw_handle(handle.0 as RawHandle) }
    }
}

fn terminate_process_handle(process: HANDLE) {
    unsafe {
        let _ = TerminateProcess(process, 1);
    }
}

pub struct SpawnedSession {
    pub job: SessionJob,
    pub process: OwnedProcess,
    pub pid: u32,
    pub stdout: Option<File>,
    pub stderr: Option<File>,
}

pub fn spawn_cmd_session(
    working_directory: &str,
    command: &str,
    pipe_stdout: bool,
    pipe_stderr: bool,
) -> Result<SpawnedSession, String> {
    let job = SessionJob::new()?;

    let mut stdout_pipe = if pipe_stdout {
        Some(PipePair::new()?)
    } else {
        None
    };
    let mut stderr_pipe = if pipe_stderr {
        Some(PipePair::new()?)
    } else {
        None
    };

    let cmdline = format!("cmd /C {command}");
    let mut cmdline_wide = os_wide(&cmdline);
    let cwd_wide = os_wide_path(Path::new(working_directory));
    let mut pi = PROCESS_INFORMATION::default();

    let mut inherited_handles = Vec::with_capacity(2);
    if let Some(pipe) = stdout_pipe.as_ref() {
        inherited_handles.push(pipe.write_handle());
    }
    if let Some(pipe) = stderr_pipe.as_ref() {
        inherited_handles.push(pipe.write_handle());
    }

    let create_result = if inherited_handles.is_empty() {
        let mut si = STARTUPINFOW::default();
        si.cb = size_of::<STARTUPINFOW>() as u32;
        unsafe {
            CreateProcessW(
                None,
                Some(windows::core::PWSTR(cmdline_wide.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_SUSPENDED | CREATE_NO_WINDOW,
                None,
                windows::core::PCWSTR(cwd_wide.as_ptr()),
                &si,
                &mut pi,
            )
        }
    } else {
        let attributes = ProcThreadAttributeList::for_handle_list(&inherited_handles)?;
        let mut si = STARTUPINFOEXW::default();
        si.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.StartupInfo.hStdOutput = stdout_pipe
            .as_ref()
            .map(|p| p.write_handle())
            .unwrap_or_default();
        si.StartupInfo.hStdError = stderr_pipe
            .as_ref()
            .map(|p| p.write_handle())
            .unwrap_or_default();
        si.lpAttributeList = attributes.list;

        // `attributes` must stay alive until CreateProcessW returns.
        let result = unsafe {
            CreateProcessW(
                None,
                Some(windows::core::PWSTR(cmdline_wide.as_mut_ptr())),
                None,
                None,
                true,
                CREATE_SUSPENDED | CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT,
                None,
                windows::core::PCWSTR(cwd_wide.as_ptr()),
                &si.StartupInfo,
                &mut pi,
            )
        };
        drop(attributes);
        result
    };

    if let Err(e) = create_result {
        return Err(format!("CreateProcessW: {}", e));
    }

    if let Some(p) = stdout_pipe.as_mut() {
        p.close_write();
    }
    if let Some(p) = stderr_pipe.as_mut() {
        p.close_write();
    }

    let process_handle = pi.hProcess;
    let thread_handle = pi.hThread;
    let pid = unsafe { GetProcessId(process_handle) };

    if let Err(e) = job.assign_process(process_handle) {
        terminate_process_handle(process_handle);
        unsafe {
            let _ = CloseHandle(process_handle);
            let _ = CloseHandle(thread_handle);
        }
        return Err(e);
    }

    unsafe {
        if ResumeThread(thread_handle) == u32::MAX {
            terminate_process_handle(process_handle);
            let _ = CloseHandle(process_handle);
            let _ = CloseHandle(thread_handle);
            return Err("ResumeThread failed".to_string());
        }
        let _ = CloseHandle(thread_handle);
    }

    let stdout = stdout_pipe.map(PipePair::into_read_file);
    let stderr = stderr_pipe.map(PipePair::into_read_file);

    Ok(SpawnedSession {
        job,
        process: OwnedProcess(WinHandle::new(process_handle)),
        pid,
        stdout,
        stderr,
    })
}

pub fn is_pid_in_job(pid: u32, job: HANDLE) -> bool {
    let process = match unsafe { OpenProcess(PROCESS_QUERY_INFORMATION, false, pid) } {
        Ok(h) => h,
        Err(_) => return false,
    };
    let mut in_job = false.into();
    let ok = unsafe { IsProcessInJob(process, Some(job), &mut in_job) }.is_ok();
    unsafe {
        let _ = CloseHandle(process);
    }
    ok && in_job.as_bool()
}

fn os_wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s).encode_wide().chain([0]).collect()
}

fn os_wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn piped_spawn_uses_extended_handle_list_and_captures_output() {
        let mut spawned = spawn_cmd_session(".", "echo xchen-handle-list", true, true)
            .expect("spawn with explicit inherited handle list");
        let (exit_code, failed) = spawned.process.wait();
        assert!(!failed, "process wait should succeed");
        assert_eq!(exit_code, Some(0));

        let mut stdout = String::new();
        spawned
            .stdout
            .as_mut()
            .expect("stdout pipe")
            .read_to_string(&mut stdout)
            .expect("read stdout");
        assert!(stdout.contains("xchen-handle-list"), "stdout={stdout:?}");
    }
}
