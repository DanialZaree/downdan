use std::sync::Arc;

use log::warn;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, RunEvent,
};

mod aria2;
mod bridge;
mod client;
mod command;
mod commands;
mod emitter;
mod engine;
mod file;
mod manager;
mod models;
mod registry;
mod repository;
mod worker;

#[macro_use]
mod macros;

use crate::engine::{sidecar, spawn_sync_loop, EngineState};
use crate::registry::Registry;

fn setup_system_tray(app: &tauri::App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "Show danDown", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit danDown", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id("danDown-tray")
        .menu(&menu)
        .tooltip("danDown Download Manager")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                let _ = dispatch!(registry, CloseRequested);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

#[tokio::main]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    dotenvy::dotenv().ok();
    env_logger::builder()
        .format_module_path(false)
        .format_level(false)
        .format_timestamp(None)
        .init();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            command::add_new_download,
            command::add_batch_downloads,
            command::import_dropped_files,
            command::update_download_url,
            command::check_aria2_status,
            command::get_download_list,
            command::resume_download,
            command::pause_download,
            command::remove_download,
            commands::engine_get_version,
            commands::engine_add_uri,
            commands::engine_add_torrent,
            commands::engine_pause,
            commands::engine_unpause,
            commands::engine_remove,
            commands::engine_tell_active,
            commands::engine_tell_waiting,
            commands::engine_tell_stopped,
            commands::engine_get_global_stat,
            commands::engine_change_global_option,
            commands::engine_set_remote_endpoint,
            commands::engine_use_local_sidecar,
        ])
        .setup(move |app| {
            let app_handle = app.app_handle().clone();

            if let Err(err) = setup_system_tray(app) {
                warn!("failed to initialize system tray: {err}");
            }

            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async move {
                    Registry::new(app_handle.clone()).await;

                    match sidecar::start_sidecar(&app_handle).await {
                        Ok(engine_state) => {
                            aria2::register_sidecar_client(engine_state.client.clone()).await;
                            spawn_sync_loop(app_handle.clone(), Arc::clone(&engine_state));
                            app_handle.manage(engine_state);
                        }
                        Err(err) => {
                            warn!("aria2c sidecar unavailable at startup: {err}");
                        }
                    }
                });
            });

            bridge::spawn_browser_bridge();

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Minimize to system tray so active downloads continue in the background.
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let RunEvent::Exit = event {
            if let Some(engine_state) = app_handle.try_state::<Arc<EngineState>>() {
                let state = Arc::clone(&engine_state);
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async move {
                        state.shutdown_gracefully().await;
                    });
                });
            }
        }
    });
}
