//! F003 spike: Windows Job Object coverage for cmd.exe launch chains.
//! Run: cargo run --manifest-path src-tauri/spike/win_job_spike/Cargo.toml

use std::ffi::c_void;
use std::mem::size_of;
use std::thread;
use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JobObjectExtendedLimitInformation,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, SetInformationJobObject,
};
use windows::Win32::System::Threading::{
    CreateProcessW, OpenProcess, PROCESS_INFORMATION, PROCESS_QUERY_INFORMATION, ResumeThread,
    STARTUPINFOW, CREATE_NO_WINDOW, CREATE_SUSPENDED,
};

fn main() {
    println!("=== F003 Windows Job Object Spike ===\n");

    let scenarios = [
        ("spawn_then_assign", SpawnMode::SpawnThenAssign),
        ("create_suspended_assign_resume", SpawnMode::CreateSuspendedAssignResume),
        ("spawn_delay_5ms_assign", SpawnMode::SpawnDelayAssign(5)),
        ("spawn_delay_50ms_assign", SpawnMode::SpawnDelayAssign(50)),
    ];

    let commands = [
        ("cmd_node_interval", "cmd /C node -e \"setInterval(()=>{}, 1e6)\""),
        ("cmd_npm_version", "cmd /C npm --version"),
        ("cmd_ping", "cmd /C ping -n 30 127.0.0.1"),
    ];

    for (mode_name, mode) in scenarios {
        println!("--- Mode: {} ---", mode_name);
        for (label, cmdline) in commands {
            match run_scenario(mode, cmdline) {
                Ok(report) => print_report(label, &report),
                Err(e) => println!("[{}] SKIP/ERR: {}\n", label, e),
            }
        }
        println!();
    }

    println!("--- Long-running npm script via cmd ---");
    if let Ok(r) = run_scenario(
        SpawnMode::CreateSuspendedAssignResume,
        "cmd /C npm exec --yes -- node -e \"setInterval(()=>{}, 1e6)\"",
    ) {
        print_report("npm_exec_node", &r);
    } else {
        println!("npm_exec_node: skipped");
    }

    if let Ok(r) = java_scenario() {
        println!(
            "--- cmd_java (CREATE_SUSPENDED) descendants_in_job={}/{} terminate_ok={} ---",
            r.descendants_in_job.iter().filter(|b| **b).count(),
            r.descendants_in_job.len(),
            r.terminate_job_killed_all
        );
    } else {
        println!("--- cmd_java: skipped (java not on PATH or spawn failed) ---");
    }

    println!("\n--- Isolation: unrelated ping survives job terminate ---");
    if let Ok(r) = isolation_test() {
        println!(
            "unrelated_ping_survived={} job_children_killed={}",
            r.unrelated_survived, r.job_killed
        );
    }
}

#[derive(Clone, Copy)]
enum SpawnMode {
    SpawnThenAssign,
    CreateSuspendedAssignResume,
    SpawnDelayAssign(u64),
}

struct ScenarioReport {
    root_pid: u32,
    assign_ok: bool,
    root_in_job: bool,
    descendant_pids: Vec<u32>,
    descendants_in_job: Vec<bool>,
    terminate_job_killed_all: bool,
}

struct IsolationReport {
    unrelated_survived: bool,
    job_killed: bool,
}

fn print_report(label: &str, r: &ScenarioReport) {
    let in_job = r.descendants_in_job.iter().filter(|b| **b).count();
    let total = r.descendants_in_job.len();
    println!(
        "[{}] root_pid={} assign_ok={} root_in_job={} descendants={}/{} in_job terminate_ok={}",
        label,
        r.root_pid,
        r.assign_ok,
        r.root_in_job,
        in_job,
        total,
        r.terminate_job_killed_all
    );
    if !r.descendant_pids.is_empty() {
        println!(
            "  descendant_pids={:?} in_job={:?}",
            r.descendant_pids,
            r.descendants_in_job
        );
    }
}

fn run_scenario(mode: SpawnMode, cmdline: &str) -> Result<ScenarioReport, String> {
    let job = create_session_job()?;
    let (root_pid, root_handle, thread_handle) = spawn_cmd(mode, cmdline)?;
    if let SpawnMode::SpawnDelayAssign(ms) = mode {
        thread::sleep(Duration::from_millis(ms));
    }
    let assign_ok = assign_to_job(job, root_handle).is_ok();
    if let Some(th) = thread_handle {
        unsafe {
            let _ = ResumeThread(th);
            let _ = CloseHandle(th);
        }
    }
    unsafe {
        let _ = CloseHandle(root_handle);
    }

    thread::sleep(Duration::from_millis(800));

    let root_in_job = is_pid_in_job(root_pid, job);
    let descendant_pids = collect_descendants(root_pid);
    let descendants_in_job = descendant_pids
        .iter()
        .map(|pid| is_pid_in_job(*pid, job))
        .collect::<Vec<_>>();

    let terminate_job_killed_all = terminate_job_and_verify(job, root_pid, &descendant_pids);

    Ok(ScenarioReport {
        root_pid,
        assign_ok,
        root_in_job,
        descendant_pids,
        descendants_in_job,
        terminate_job_killed_all,
    })
}

fn java_scenario() -> Result<ScenarioReport, String> {
    run_scenario(
        SpawnMode::CreateSuspendedAssignResume,
        r"cmd /C java -version",
    )
}

fn create_session_job() -> Result<HANDLE, String> {
    let job = unsafe { CreateJobObjectW(None, None) }
        .map_err(|e| format!("CreateJobObjectW: {}", e))?;
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
    unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const c_void,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    }
    .map_err(|e| format!("SetInformationJobObject: {}", e))?;
    Ok(job)
}

fn spawn_cmd(mode: SpawnMode, cmdline: &str) -> Result<(u32, HANDLE, Option<HANDLE>), String> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = std::ffi::OsStr::new(cmdline)
        .encode_wide()
        .chain([0])
        .collect();
    let mut si = STARTUPINFOW::default();
    si.cb = size_of::<STARTUPINFOW>() as u32;
    let mut pi = PROCESS_INFORMATION::default();
    let flags = match mode {
        SpawnMode::SpawnThenAssign | SpawnMode::SpawnDelayAssign(_) => CREATE_NO_WINDOW,
        SpawnMode::CreateSuspendedAssignResume => CREATE_SUSPENDED | CREATE_NO_WINDOW,
    };
    unsafe {
        CreateProcessW(
            None,
            Some(windows::core::PWSTR(wide.as_ptr() as *mut _)),
            None,
            None,
            false,
            flags,
            None,
            None,
            &mut si,
            &mut pi,
        )
    }
    .map_err(|e| format!("CreateProcessW: {}", e))?;

    let pid = pi.dwProcessId;
    let thread = match mode {
        SpawnMode::CreateSuspendedAssignResume => Some(pi.hThread),
        SpawnMode::SpawnThenAssign | SpawnMode::SpawnDelayAssign(_) => {
            unsafe {
                let _ = CloseHandle(pi.hThread);
            }
            None
        }
    };
    Ok((pid, pi.hProcess, thread))
}

fn assign_to_job(job: HANDLE, process: HANDLE) -> Result<(), String> {
    unsafe { AssignProcessToJobObject(job, process) }
        .map_err(|e| format!("AssignProcessToJobObject: {}", e))
}

fn is_pid_in_job(pid: u32, job: HANDLE) -> bool {
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

fn collect_descendants(root_pid: u32) -> Vec<u32> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );
    let mut frontier = vec![root_pid];
    let mut seen = vec![root_pid];
    let mut descendants = Vec::new();
    for _ in 0..4 {
        let mut next = Vec::new();
        for parent in &frontier {
            for (pid, p) in system.processes() {
                let child = pid.as_u32();
                if p.parent().map(|p| p.as_u32()) == Some(*parent) && !seen.contains(&child) {
                    next.push(child);
                    descendants.push(child);
                    seen.push(child);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    descendants
}

fn terminate_job_and_verify(job: HANDLE, root_pid: u32, descendants: &[u32]) -> bool {
    unsafe {
        let _ = CloseHandle(job);
    }
    thread::sleep(Duration::from_millis(500));
    !is_pid_alive(root_pid) && descendants.iter().all(|pid| !is_pid_alive(*pid))
}

fn is_pid_alive(pid: u32) -> bool {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::everything(),
    );
    system.process(Pid::from_u32(pid)).is_some()
}

fn isolation_test() -> Result<IsolationReport, String> {
    let job = create_session_job()?;
    let (root_pid, root_handle, thread_handle) =
        spawn_cmd(SpawnMode::CreateSuspendedAssignResume, r"cmd /C ping -n 60 127.0.0.1")?;
    assign_to_job(job, root_handle)?;
    if let Some(th) = thread_handle {
        unsafe {
            let _ = ResumeThread(th);
            let _ = CloseHandle(th);
        }
    }
    unsafe {
        let _ = CloseHandle(root_handle);
    }

    let (unrelated_pid, unrelated_handle, unrelated_thread) =
        spawn_cmd(SpawnMode::SpawnThenAssign, r"cmd /C ping -n 60 127.0.0.1")?;
    if let Some(th) = unrelated_thread {
        unsafe {
            let _ = CloseHandle(th);
        }
    }
    unsafe {
        let _ = CloseHandle(unrelated_handle);
    }
    thread::sleep(Duration::from_millis(300));

    unsafe {
        let _ = CloseHandle(job);
    }
    thread::sleep(Duration::from_millis(500));

    Ok(IsolationReport {
        unrelated_survived: is_pid_alive(unrelated_pid),
        job_killed: !is_pid_alive(root_pid),
    })
}
