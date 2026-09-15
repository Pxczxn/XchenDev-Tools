use xchendev_tools_lib::domain::ProcessSummary;
use xchendev_tools_lib::process_manager;
use xchendev_tools_lib::security_guard;

#[test]
fn protected_system_pid_is_rejected() {
    let summary = ProcessSummary {
        pid: 4,
        name: "System".to_string(),
        working_directory: None,
        command_line: None,
    };
    assert!(security_guard::is_protected_process(&summary));
}

#[test]
fn explorer_is_protected_by_default() {
    let summary = ProcessSummary {
        pid: 12345,
        name: "explorer.exe".to_string(),
        working_directory: Some("C:\\Windows".to_string()),
        command_line: None,
    };
    let decision = security_guard::protection_for_process(&summary);
    assert!(decision.is_protected);
    assert!(decision.reason.unwrap_or_default().contains("系统关键进程"));
}

#[test]
fn terminate_protected_process_fails_without_taskkill() {
    let err = process_manager::terminate_pid(4, false).unwrap_err();
    assert!(err.contains("PROCESS_PROTECTED"));
}

#[test]
fn dangerous_command_is_rejected() {
    let err = security_guard::validate_command_policy("cmd && del /f *").unwrap_err();
    assert!(err.contains("COMMAND_POLICY_REJECTED"));
}
