use std::{
    net::TcpListener,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use log::{debug, warn};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_shell::{process::CommandChild, process::CommandEvent, ShellExt};
use tokio::{sync::Mutex, time::sleep};
use uuid::Uuid;

use super::rpc::{Aria2Client, DownloadTask, EngineError, GlobalStat};

const DEFAULT_RPC_PORT: u16 = 6800;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineTickPayload {
    pub active: Vec<DownloadTask>,
    pub global_stat: GlobalStat,
}

pub struct EngineState {
    pub client: Aria2Client,
    pub port: u16,
    pub secret: String,
    #[allow(dead_code)]
    pub session_path: PathBuf,
    child: Mutex<Option<CommandChild>>,
    ready: AtomicBool,
    using_remote: AtomicBool,
    shutting_down: AtomicBool,
}

impl EngineState {
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire) || self.using_remote.load(Ordering::Acquire)
    }

    pub fn ensure_ready(&self) -> Result<(), EngineError> {
        if self.is_ready() {
            Ok(())
        } else {
            Err(EngineError::NotReady(
                "aria2c sidecar is still initializing".to_string(),
            ))
        }
    }

    pub async fn switch_to_remote(&self, url: String, secret: String) {
        self.using_remote.store(true, Ordering::Release);
        self.client.set_endpoint(url, secret).await;
    }

    pub async fn switch_to_local_sidecar(&self) {
        self.using_remote.store(false, Ordering::Release);
        self.client
            .set_endpoint(
                format!("http://127.0.0.1:{}/jsonrpc", self.port),
                self.secret.clone(),
            )
            .await;
    }

    /// Saves the aria2 session, requests a graceful RPC shutdown, and ensures the
    /// underlying `CommandChild` process is terminated so no orphan processes survive.
    pub async fn shutdown_gracefully(&self) {
        if self.shutting_down.swap(true, Ordering::AcqRel) {
            return;
        }

        if !self.using_remote.load(Ordering::Acquire) {
            let _ = self.client.save_session().await;
            let _ = self.client.shutdown().await;
            sleep(Duration::from_millis(150)).await;
        }

        let mut child_guard = self.child.lock().await;
        if let Some(child) = child_guard.take() {
            if let Err(err) = child.kill() {
                warn!("failed to kill aria2c sidecar child: {err}");
            }
        }
    }
}

/// Picks `preferred` if available on `127.0.0.1`, otherwise binds port `0` to let the OS
/// assign an ephemeral free localhost port.
pub fn pick_free_port(preferred: u16) -> Result<u16, EngineError> {
    if let Ok(listener) = TcpListener::bind(("127.0.0.1", preferred)) {
        let port = listener
            .local_addr()
            .map_err(|e| EngineError::Sidecar(e.to_string()))?
            .port();
        drop(listener);
        return Ok(port);
    }

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| EngineError::Sidecar(format!("failed to bind fallback port: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| EngineError::Sidecar(e.to_string()))?
        .port();
    drop(listener);
    Ok(port)
}

fn ensure_session_file(app: &AppHandle) -> Result<PathBuf, EngineError> {
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| EngineError::Sidecar(format!("failed to resolve app_local_data_dir: {e}")))?;

    std::fs::create_dir_all(&data_dir)
        .map_err(|e| EngineError::Sidecar(format!("failed to create data dir: {e}")))?;

    let session_path = data_dir.join("aria2.session");
    if !session_path.exists() {
        std::fs::File::create(&session_path).map_err(|e| {
            EngineError::Sidecar(format!("failed to create aria2.session: {e}"))
        })?;
    }

    Ok(session_path)
}

/// Spawns the bundled `aria2c` sidecar, builds `Arc<EngineState>`, and polls
/// `aria2.getVersion` with exponential backoff until the RPC server is ready.
pub async fn start_sidecar(app: &AppHandle) -> Result<Arc<EngineState>, EngineError> {
    let port = pick_free_port(DEFAULT_RPC_PORT)?;
    let secret = Uuid::new_v4().simple().to_string();
    let session_path = ensure_session_file(app)?;
    let session_str = session_path.to_string_lossy().into_owned();

    let file_allocation = if cfg!(target_os = "windows") {
        "trunc"
    } else {
        "falloc"
    };

    let args = vec![
        "--enable-rpc=true".to_string(),
        "--rpc-listen-all=false".to_string(),
        format!("--rpc-listen-port={port}"),
        format!("--rpc-secret={secret}"),
        format!("--input-file={session_str}"),
        format!("--save-session={session_str}"),
        "--save-session-interval=30".to_string(),
        "--max-connection-per-server=16".to_string(),
        "--split=16".to_string(),
        "--min-split-size=2M".to_string(),
        "--max-concurrent-downloads=5".to_string(),
        "--disk-cache=16M".to_string(),
        "--continue=true".to_string(),
        format!("--file-allocation={file_allocation}"),
    ];

    let sidecar_cmd = app
        .shell()
        .sidecar("aria2c")
        .map_err(|e| EngineError::Sidecar(format!("failed to locate aria2c sidecar: {e}")))?
        .args(args);

    let (mut rx, child) = sidecar_cmd
        .spawn()
        .map_err(|e| EngineError::Sidecar(format!("failed to spawn aria2c sidecar: {e}")))?;

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stderr(line) => {
                    let msg = String::from_utf8_lossy(&line);
                    debug!("[aria2c stderr] {msg}");
                }
                CommandEvent::Error(err) => {
                    warn!("[aria2c error] {err}");
                }
                CommandEvent::Terminated(payload) => {
                    debug!("[aria2c terminated] code={:?} signal={:?}", payload.code, payload.signal);
                    break;
                }
                _ => {}
            }
        }
    });

    let client = Aria2Client::new(port, secret.clone())?;
    let state = Arc::new(EngineState {
        client,
        port,
        secret,
        session_path,
        child: Mutex::new(Some(child)),
        ready: AtomicBool::new(false),
        using_remote: AtomicBool::new(false),
        shutting_down: AtomicBool::new(false),
    });

    wait_for_ready(&state).await?;
    Ok(state)
}

async fn wait_for_ready(state: &EngineState) -> Result<(), EngineError> {
    let mut backoff = Duration::from_millis(50);
    let max_backoff = Duration::from_secs(1);

    for attempt in 1..=15 {
        match state.client.get_version().await {
            Ok(ver) => {
                debug!(
                    "aria2c sidecar ready on port {} (v{}, attempt {})",
                    state.port, ver.version, attempt
                );
                state.ready.store(true, Ordering::Release);
                return Ok(());
            }
            Err(_) if attempt < 15 => {
                sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
            }
            Err(err) => {
                return Err(EngineError::NotReady(format!(
                    "aria2c did not respond to getVersion: {err}"
                )));
            }
        }
    }

    Err(EngineError::NotReady(
        "aria2c health check exhausted".to_string(),
    ))
}

/// Polls `tell_active` + `get_global_stat` every 500ms while active downloads exist or at
/// least one window is visible, emitting `engine://tick` to the frontend.
pub fn spawn_sync_loop(app: AppHandle, state: Arc<EngineState>) {
    tokio::spawn(async move {
        let mut had_active = false;

        loop {
            sleep(Duration::from_millis(500)).await;

            if state.shutting_down.load(Ordering::Acquire) {
                break;
            }
            if !state.is_ready() {
                continue;
            }

            let any_window_visible = app
                .webview_windows()
                .values()
                .any(|w| w.is_visible().unwrap_or(false));

            if !any_window_visible && !had_active {
                continue;
            }

            let (active_res, stat_res) =
                tokio::join!(state.client.tell_active(), state.client.get_global_stat());

            if let (Ok(active), Ok(global_stat)) = (active_res, stat_res) {
                let is_active = !active.is_empty() || global_stat.num_active > 0;
                if is_active || had_active || any_window_visible {
                    let _ = app.emit(
                        "engine://tick",
                        EngineTickPayload {
                            active,
                            global_stat,
                        },
                    );
                }
                had_active = is_active;
            }
        }
    });
}
