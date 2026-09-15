pub mod app_state;
mod audit_log;
mod command_runner;
mod config_transaction;
pub mod config_store;
pub mod domain;
mod environment_detector;
pub mod error;
mod history_retention;
mod port_manager;
pub mod process_manager;
pub mod project_scanner;
pub mod security_guard;
pub mod service_manager;
mod settings_guard;
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
            save_manual_override_safe,
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
            list_audit_events_safe,
            list_recent_errors_safe,
            list_default_protected_processes,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod ipc_contract_tests {
    use std::collections::BTreeSet;

    fn quoted_invoke_commands(source: &str) -> BTreeSet<String> {
        let mut commands = BTreeSet::new();
        let mut remaining = source;

        while let Some(invoke_pos) = remaining.find("invoke") {
            let after_invoke = &remaining[invoke_pos + "invoke".len()..];
            let Some(paren_pos) = after_invoke.find('(') else {
                break;
            };
            let args = &after_invoke[paren_pos + 1..];
            let Some(quote_pos) = args.find('"') else {
                remaining = args;
                continue;
            };
            let after_quote = &args[quote_pos + 1..];
            let Some(end) = after_quote.find('"') else {
                break;
            };

            commands.insert(after_quote[..end].to_string());
            remaining = &after_quote[end + 1..];
        }

        commands
    }

    fn registered_handler_commands(source: &str) -> BTreeSet<String> {
        let Some(start) = source.find("tauri::generate_handler![") else {
            panic!("generate_handler block not found");
        };
        let block = &source[start..];
        let Some(end) = block.find("])\n") else {
            panic!("generate_handler block end not found");
        };

        block[..end]
            .lines()
            .skip(1)
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| line.trim_end_matches(',').to_string())
            .filter(|line| {
                line.chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            })
            .collect()
    }

    #[test]
    fn every_frontend_invoke_is_registered_in_tauri_handler() {
        let client = include_str!("../../src/ipc/client.ts");
        let backend = include_str!("lib.rs");
        let invoked = quoted_invoke_commands(client);
        let registered = registered_handler_commands(backend);

        let missing: Vec<_> = invoked.difference(&registered).cloned().collect();
        assert!(
            missing.is_empty(),
            "frontend invokes Tauri commands that are not registered: {missing:?}"
        );
    }
}
