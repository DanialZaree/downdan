use std::{
    io::Write,
    net::{SocketAddr, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use log::warn;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, RunEvent,
};

static TRAY_AVAILABLE: AtomicBool = AtomicBool::new(false);

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

fn notify_existing_instance(silent_wake: bool) -> bool {
    let addr: SocketAddr = "127.0.0.1:17890".parse().unwrap();
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(150)) else {
        return false;
    };
    if !silent_wake {
        let _ = stream.write_all(
            b"POST /show HTTP/1.1\r\nHost: 127.0.0.1:17890\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
    }
    true
}

#[cfg(target_os = "windows")]
fn register_url_protocol() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let exe_str = exe.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let cmd_val = format!("\"{exe_str}\" \"%1\"");

    let _ = std::process::Command::new("reg.exe")
        .args([
            "add",
            r"HKCU\Software\Classes\dandown",
            "/ve",
            "/d",
            "URL:danDown Protocol",
            "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let _ = std::process::Command::new("reg.exe")
        .args([
            "add",
            r"HKCU\Software\Classes\dandown",
            "/v",
            "URL Protocol",
            "/d",
            "",
            "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let _ = std::process::Command::new("reg.exe")
        .args([
            "add",
            r"HKCU\Software\Classes\dandown\shell\open\command",
            "/ve",
            "/d",
            &cmd_val,
            "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

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
    } else {
        builder = builder.icon(tauri::include_image!("icons/32x32.png"));
    }

    builder.build(app)?;
    Ok(())
}

#[tokio::main]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    let args: Vec<String> = std::env::args().collect();
    let is_silent_wake = args
        .iter()
        .any(|a| a.starts_with("dandown://") || a == "--background");

    if notify_existing_instance(is_silent_wake) {
        return;
    }

    #[cfg(target_os = "windows")]
    register_url_protocol();

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
            command::open_extension_folder,
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

            match setup_system_tray(app) {
                Ok(()) => TRAY_AVAILABLE.store(true, Ordering::SeqCst),
                Err(err) => warn!("failed to initialize system tray: {err}"),
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

            if is_silent_wake && TRAY_AVAILABLE.load(Ordering::SeqCst) {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if TRAY_AVAILABLE.load(Ordering::SeqCst) {
                    // Minimize to system tray so active downloads continue in the background.
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    let _ = dispatch!(registry, CloseRequested);
                }
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
