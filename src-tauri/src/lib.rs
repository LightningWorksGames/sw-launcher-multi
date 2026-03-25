mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::select_install_path,
            commands::check_updates,
            commands::download_game,
            commands::launch_game,
            commands::start_sso_login,
            commands::get_stored_auth,
            commands::verify_token,
            commands::logout,
            commands::fetch_launcher_config,
            commands::save_launcher_config,
            commands::fetch_slides,
            commands::get_cached_slides,
            commands::upload_slide,
            commands::delete_slide,
            commands::save_slide_order,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
