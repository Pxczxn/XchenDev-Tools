pub mod app_state;
mod audit_log;
mod command_runner;
pub mod config_store;
pub mod domain;
mod environment_detector;
pub mod error;
mod port_manager;
pub mod process_manager;
pub mod project_scanner;
pub mod security_guard;
pub mod service_manager;
pub mod tauri_api;

use app_state::AppState;
use tauri_api::*;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = AppState::new();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            health_check,
            list_runtime_items_safe,
            list_environment_candidates,
            save_manual_override,
            inspect_port,
            issue_process_termination_confirmation_safe,
            terminate_process_safe,
            inspect_directory_processes,
            terminate_directory_process_safe,
            scan_project_directory,
            save_launch_profile_safe,
            remove_launch_profile_safe,
            list_launch_profiles,
            issue_launch_confirmation_safe,
            start_launch_profile_safe,
            stop_launch_session,
            project_id_for_path,
            list_projects,
            upsert_project,
            remove_project,
            get_config_paths,
            export_app_config,
            import_app_config_safe,
            import_app_config_from_path_safe,
            export_app_config_to_path,
            list_active_launch_sessions,
            list_managed_services,
            issue_service_control_confirmation_safe,
            control_windows_service_safe,
            get_app_settings,
            save_app_settings_safe,
            list_audit_events,
            list_recent_errors,
            list_default_protected_processes,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
