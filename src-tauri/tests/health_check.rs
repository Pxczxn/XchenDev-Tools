#[test]
fn health_check_returns_version_and_platform() {
    std::env::remove_var("XCHEN_IPC_SIMULATE_FAILURE");
    let response = xchendev_tools_lib::tauri_api::health_check().expect("health");
    assert!(!response.app_version.is_empty());
    assert_eq!(response.ipc_status, "ONLINE");
}

#[test]
fn health_check_simulated_failure() {
    std::env::set_var("XCHEN_IPC_SIMULATE_FAILURE", "1");
    let result = xchendev_tools_lib::tauri_api::health_check();
    std::env::remove_var("XCHEN_IPC_SIMULATE_FAILURE");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), "IPC_NOT_READY");
}
