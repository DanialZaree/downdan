use std::{collections::HashMap, sync::Arc};

use tauri::State;

use crate::engine::{
    Aria2Version, DownloadOptions, DownloadTask, EngineError, EngineState, GlobalStat,
};

#[tauri::command]
pub async fn engine_get_version(state: State<'_, Arc<EngineState>>) -> Result<Aria2Version, EngineError> {
    state.ensure_ready()?;
    state.client.get_version().await
}

#[tauri::command]
pub async fn engine_add_uri(
    state: State<'_, Arc<EngineState>>,
    uris: Vec<String>,
    options: Option<DownloadOptions>,
) -> Result<String, EngineError> {
    state.ensure_ready()?;
    state.client.add_uri(uris, options).await
}

#[tauri::command]
pub async fn engine_add_torrent(
    state: State<'_, Arc<EngineState>>,
    torrent_base64: String,
    options: Option<DownloadOptions>,
) -> Result<String, EngineError> {
    state.ensure_ready()?;
    state.client.add_torrent(torrent_base64, options).await
}

#[tauri::command]
pub async fn engine_pause(
    state: State<'_, Arc<EngineState>>,
    gid: String,
) -> Result<String, EngineError> {
    state.ensure_ready()?;
    state.client.pause(gid).await
}

#[tauri::command]
pub async fn engine_unpause(
    state: State<'_, Arc<EngineState>>,
    gid: String,
) -> Result<String, EngineError> {
    state.ensure_ready()?;
    state.client.unpause(gid).await
}

#[tauri::command]
pub async fn engine_remove(
    state: State<'_, Arc<EngineState>>,
    gid: String,
) -> Result<String, EngineError> {
    state.ensure_ready()?;
    state.client.remove(gid).await
}

#[tauri::command]
pub async fn engine_tell_active(
    state: State<'_, Arc<EngineState>>,
) -> Result<Vec<DownloadTask>, EngineError> {
    state.ensure_ready()?;
    state.client.tell_active().await
}

#[tauri::command]
pub async fn engine_tell_waiting(
    state: State<'_, Arc<EngineState>>,
    offset: i64,
    num: i64,
) -> Result<Vec<DownloadTask>, EngineError> {
    state.ensure_ready()?;
    state.client.tell_waiting(offset, num).await
}

#[tauri::command]
pub async fn engine_tell_stopped(
    state: State<'_, Arc<EngineState>>,
    offset: i64,
    num: i64,
) -> Result<Vec<DownloadTask>, EngineError> {
    state.ensure_ready()?;
    state.client.tell_stopped(offset, num).await
}

#[tauri::command]
pub async fn engine_get_global_stat(
    state: State<'_, Arc<EngineState>>,
) -> Result<GlobalStat, EngineError> {
    state.ensure_ready()?;
    state.client.get_global_stat().await
}

#[tauri::command]
pub async fn engine_change_global_option(
    state: State<'_, Arc<EngineState>>,
    options: HashMap<String, String>,
) -> Result<String, EngineError> {
    state.ensure_ready()?;
    state.client.change_global_option(options).await
}

#[tauri::command]
pub async fn engine_set_remote_endpoint(
    state: State<'_, Arc<EngineState>>,
    url: String,
    secret: String,
) -> Result<(), EngineError> {
    state.switch_to_remote(url, secret).await;
    Ok(())
}

#[tauri::command]
pub async fn engine_use_local_sidecar(
    state: State<'_, Arc<EngineState>>,
) -> Result<(), EngineError> {
    state.switch_to_local_sidecar().await;
    Ok(())
}
