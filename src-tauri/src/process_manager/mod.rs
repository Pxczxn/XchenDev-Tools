use crate::domain::{ProcessSummary, ProtectionDecision};
use std::ffi::OsString;

fn format_cmd(parts: &[OsString]) -> String {
    parts
        .iter()
        .map(|s| s.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}
use crate::security_guard::{protection_for_process, snapshot_digest};
use crate::security_guard;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

#[cfg(windows)]
pub struct ProcessIdentityGuard(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for ProcessIdentityGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(not(windows))]
pub struct ProcessIdentityGuard;

/// Keep the current process object alive while a PID-based operation is in flight.
/// On Windows, a PID cannot be reused until all handles to the terminated process are closed.
pub fn pin_process_identity(pid: u32) -> Result<ProcessIdentityGuard, String> {
    if process_start_time_secs(pid).is_none() {
        return Err("PROCESS_NOT_FOUND:进程不存在".to_string());
    }

    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
            .map_err(|e| format!("TERMINATE_DENIED:无法锁定目标进程身份 {}", e))?;
        Ok(ProcessIdentityGuard(handle))
    }

    #[cfg(not(windows))]
    {
        Ok(ProcessIdentityGuard)
    }
}

/// Process start time in seconds since UNIX epoch (sysinfo), if `pid` currently exists.
pub fn process_start_time_secs(pid: u32) -> Option<u64> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::everything(),
    );
    system
        .process(Pid::from_u32(pid))
        .map(|process| process.start_time())
}

pub fn find_process_summary(pid: u32) -> Option<ProcessSummary> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::everything(),
    );
    let process = system.process(Pid::from_u32(pid))?;
    let name = process.name().to_string_lossy().to_string();
    let cwd = process.cwd().map(|p| p.to_string_lossy().to_string());
    let cmd = format_cmd(process.cmd());
    Some(ProcessSummary {
        pid,
        name,
        working_directory: cwd,
        command_line: if cmd.is_empty() { None } else { Some(cmd) },
    })
}

pub fn list_all_summaries() -> Vec<ProcessSummary> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );
    system
        .processes()
        .iter()
        .map(|(pid, process)| {
            let name = process.name().to_string_lossy().to_string();
            let cwd = process.cwd().map(|p| p.to_string_lossy().to_string());
            let cmd = format_cmd(process.cmd());
            ProcessSummary {
                pid: pid.as_u32(),
                name,
                working_directory: cwd,
                command_line: if cmd.is_empty() { None } else { Some(cmd) },
            }
        })
        .collect()
}

pub fn verify_process_snapshot(
    pid: u32,
    expected_name: &str,
    expected_cwd: Option<&str>,
) -> Result<(), String> {
    let summary = find_process_summary(pid).ok_or_else(|| "PROCESS_NOT_FOUND:进程不存在".to_string())?;
    if summary.name.to_lowercase() != expected_name.to_lowercase() {
        return Err("PROCESS_SNAPSHOT_MISMATCH:进程名不匹配".to_string());
    }
    if let Some(expected) = expected_cwd {
        match &summary.working_directory {
            Some(actual) if actual.eq_ignore_ascii_case(expected) => {}
            _ => return Err("PROCESS_SNAPSHOT_MISMATCH:工作目录不匹配".to_string()),
        }
    }
    Ok(())
}

pub fn terminate_pid(pid: u32, force: bool) -> Result<(), String> {
    terminate_pid_with_extra(pid, force, &[])
}

pub fn terminate_pid_with_extra(
    pid: u32,
    force: bool,
    extra_protected: &[String],
) -> Result<(), String> {
    let summary = find_process_summary(pid).ok_or_else(|| "PROCESS_NOT_FOUND:进程不存在".to_string())?;
    let protection = security_guard::protection_for_process_with_extra(&summary, extra_protected);
    if protection.is_protected {
        return Err("PROCESS_PROTECTED:目标进程受保护".to_string());
    }
    let status = if force {
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status()
    } else {
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string()])
            .status()
    };
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err("TERMINATE_FAILED:终止失败".to_string()),
        Err(e) => Err(format!("TERMINATE_DENIED:{}", e)),
    }
}

pub fn digest_for(pid: u32, name: &str, cwd: Option<&str>) -> String {
    // Bind directory-operation snapshots to process creation identity as well as PID/name/cwd.
    // If Windows reuses the PID before the user confirms termination, the recomputed digest changes.
    let start_time = process_start_time_secs(pid)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "missing".to_string());
    let identity_name = format!("{}|start={}", name, start_time);
    snapshot_digest(pid, &identity_name, cwd)
}

pub fn protection(summary: &ProcessSummary) -> ProtectionDecision {
    protection_for_process(summary)
}

pub fn protection_with_extra(summary: &ProcessSummary, extra: &[String]) -> ProtectionDecision {
    security_guard::protection_for_process_with_extra(summary, extra)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_current_process_identity_succeeds() {
        let _guard = pin_process_identity(std::process::id()).expect("pin current process");
    }

    #[test]
    fn pin_missing_process_identity_fails() {
        assert!(pin_process_identity(u32::MAX).is_err());
    }
}
