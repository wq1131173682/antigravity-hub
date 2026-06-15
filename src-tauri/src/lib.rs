mod models;
mod modules;
mod commands;
mod utils;
pub mod error;
pub mod constants;

use modules::logger;
use tracing::info;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logger
    logger::init_logger();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            info!("App starting up...");

            // Initialize log bridge with app handle for debug console
            modules::log_bridge::init_log_bridge(app.handle().clone());

            // Initialize quota window tracker
            modules::quota_window::initialize();

            // Initialize proxy port and host from config
            if let Ok(cfg) = modules::config::load_app_config() {
                modules::proxy::init_proxy_port(cfg.proxy_port);
                modules::proxy::init_proxy_host(cfg.proxy_host);
            }

            // Start background scheduler for cleanup
            modules::scheduler::start_scheduler();

            // Build tray menu with i18n support
            use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};

            // Detect system language for tray menu translations
            let sys_lang = {
                #[cfg(target_os = "windows")]
                {
                    std::env::var("VSLang")
                        .or_else(|_| std::env::var("LANG"))
                        .unwrap_or_else(|_| "zh-CN".to_string())
                }
                #[cfg(not(target_os = "windows"))]
                {
                    std::env::var("LANG").unwrap_or_else(|_| "zh_CN.UTF-8".to_string())
                }
            };
            let texts = modules::i18n::get_tray_texts(&sys_lang);

            let show = MenuItem::with_id(app, "show", &texts.show_window, true, None::<&str>)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let switch_next = MenuItem::with_id(app, "switch_next", &texts.switch_next, true, None::<&str>)?;
            let refresh = MenuItem::with_id(app, "refresh", &texts.refresh_current, true, None::<&str>)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", &texts.quit, true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &sep1, &switch_next, &refresh, &sep2, &quit])?;

            // Create tray icon with dedicated icon and click-to-show support
            let tray = tauri::tray::TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .icon_as_template(true)
                .tooltip("Antigravity Hub")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "switch_next" => {
                            info!("Tray: switch next account requested");
                            let _ = app.emit("tray:switch_next", ());
                        }
                        "refresh" => {
                            info!("Tray: refresh quota requested");
                            let _ = app.emit("tray:refresh_quota", ());
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;
            app.manage(tray);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Platform management
            commands::list_platforms,
            commands::add_platform,
            commands::update_platform,
            commands::delete_platform,
            commands::reorder_platforms,
            // Model management
            commands::list_models,
            commands::add_model,
            commands::update_model,
            commands::delete_model,
            // Key-Model associations
            commands::get_keys_for_model,
            commands::get_models_for_key,
            commands::associate_key_with_model,
            commands::disassociate_key_from_model,
            // API Key management
            commands::list_keys,
            commands::add_key,
            commands::update_key,
            commands::delete_key,
            commands::enable_key,
            commands::disable_key,
            commands::set_key_status,
            // Quota window tracking (per model+key)
            commands::record_api_call_cmd,
            commands::record_429_error_cmd,
            commands::record_500_error_cmd,
            commands::get_quota_window_status,
            commands::get_key_usage,
            commands::get_model_usage,
            commands::set_auto_switch_cmd,
            commands::get_auto_switch_cmd,
            commands::remove_quota_tracker,
            commands::clean_expired_disabled_cmd,
            // Proxy control
            commands::start_proxy,
            commands::stop_proxy,
            commands::get_proxy_status,
            // Config
            commands::load_config,
            commands::save_config,
            commands::set_proxy_port,
            commands::set_proxy_host,
            // Window controls
            commands::minimize_window,
            commands::toggle_maximize_window,
            commands::close_window,
            // Utilities
            commands::save_text_file,
            commands::read_text_file,
            commands::clear_log_cache,
            commands::open_data_folder,
            commands::get_data_dir_path,
            commands::show_main_window,
            commands::set_window_theme,
            // Utility
            commands::get_lan_ip,
            // Debug console
            modules::log_bridge::enable_debug_console,
            modules::log_bridge::disable_debug_console,
            modules::log_bridge::is_debug_console_enabled,
            modules::log_bridge::get_debug_console_logs,
            modules::log_bridge::clear_debug_console_logs,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            match event {
                tauri::RunEvent::WindowEvent { label, event: window_event, .. } => {
                    if label == "main" {
                        if let tauri::WindowEvent::CloseRequested { .. } = window_event {
                            info!("Minimizing to tray instead of closing");
                            if let Some(window) = app_handle.get_webview_window("main") {
                                let _ = window.hide();
                            }
                        }
                    }
                }
                tauri::RunEvent::Exit => {
                    tracing::info!("Application exiting");
                }
                _ => {}
            }
        });
}
