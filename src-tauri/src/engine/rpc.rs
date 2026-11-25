use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use reqwest::Client as ReqwestClient;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("HTTP transport error: {0}")]
    Transport(#[from] reqwest::Error),

    #[error("JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("aria2 RPC error ({code}): {message}")]
    Rpc { code: i64, message: String },

    #[error("aria2 RPC response missing result field")]
    MissingResult,

    #[error("sidecar lifecycle error: {0}")]
    Sidecar(String),

    #[error("engine is not ready: {0}")]
    NotReady(String),
}

impl Serialize for EngineError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub out: Option<String>,
    /// Raw HTTP headers, e.g. `["Cookie: a=b", "Referer: https://...", "User-Agent: ..."]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split: Option<u32>,
    #[serde(
        rename = "max-connection-per-server",
        alias = "maxConnectionPerServer",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_connection_per_server: Option<u32>,
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, Value>,
}

impl DownloadOptions {
    pub fn to_rpc_map(&self) -> serde_json::Map<String, Value> {
        let mut map = self.extra.clone();
        if let Some(dir) = &self.dir {
            map.insert("dir".to_string(), Value::String(dir.clone()));
        }
        if let Some(out) = &self.out {
            map.insert("out".to_string(), Value::String(out.clone()));
        }
        if let Some(headers) = &self.header {
            if !headers.is_empty() {
                map.insert(
                    "header".to_string(),
                    Value::Array(headers.iter().cloned().map(Value::String).collect()),
                );
            }
        }
        if let Some(split) = self.split {
            map.insert("split".to_string(), Value::String(split.to_string()));
        }
        if let Some(max_conn) = self.max_connection_per_server {
            map.insert(
                "max-connection-per-server".to_string(),
                Value::String(max_conn.to_string()),
            );
        }
        map
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskFile {
    pub index: String,
    pub path: String,
    pub length: u64,
    pub completed_length: u64,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTask {
    pub gid: String,
    pub status: String,
    pub total_length: u64,
    pub completed_length: u64,
    pub upload_length: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub connections: u64,
    pub progress_percent: f64,
    pub eta_seconds: Option<u64>,
    pub dir: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub files: Vec<TaskFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GlobalStat {
    pub download_speed: u64,
    pub upload_speed: u64,
    pub num_active: u64,
    pub num_waiting: u64,
    pub num_stopped: u64,
    pub num_stopped_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Aria2Version {
    pub version: String,
    #[serde(default)]
    pub enabled_features: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTaskFile {
    #[serde(default)]
    index: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    length: String,
    #[serde(default)]
    completed_length: String,
    #[serde(default)]
    selected: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDownloadTask {
    gid: String,
    status: String,
    #[serde(default)]
    total_length: String,
    #[serde(default)]
    completed_length: String,
    #[serde(default)]
    upload_length: String,
    #[serde(default)]
    download_speed: String,
    #[serde(default)]
    upload_speed: String,
    #[serde(default)]
    connections: String,
    #[serde(default)]
    dir: String,
    error_code: Option<String>,
    error_message: Option<String>,
    #[serde(default)]
    files: Vec<RawTaskFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawGlobalStat {
    #[serde(default)]
    download_speed: String,
    #[serde(default)]
    upload_speed: String,
    #[serde(default)]
    num_active: String,
    #[serde(default)]
    num_waiting: String,
    #[serde(default)]
    num_stopped: String,
    #[serde(default)]
    num_stopped_total: String,
}

#[derive(Debug, Deserialize)]
struct RpcErrorPayload {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
struct RpcEnvelope<T> {
    result: Option<T>,
    error: Option<RpcErrorPayload>,
}

#[derive(Debug, Clone)]
struct RpcEndpoint {
    url: String,
    secret: String,
}

#[derive(Clone)]
pub struct Aria2Client {
    http: ReqwestClient,
    endpoint: Arc<RwLock<RpcEndpoint>>,
    request_id: Arc<AtomicU64>,
}

impl std::fmt::Debug for Aria2Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Aria2Client").finish_non_exhaustive()
    }
}

impl Aria2Client {
    pub fn new(port: u16, secret: impl Into<String>) -> Result<Self, EngineError> {
        let http = ReqwestClient::builder()
            .no_proxy()
            .pool_max_idle_per_host(8)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()?;

        Ok(Self {
            http,
            endpoint: Arc::new(RwLock::new(RpcEndpoint {
                url: format!("http://127.0.0.1:{port}/jsonrpc"),
                secret: secret.into(),
            })),
            request_id: Arc::new(AtomicU64::new(1)),
        })
    }

    /// Switches the RPC endpoint and secret at runtime (e.g. to control a remote NAS aria2 daemon).
    pub async fn set_endpoint(&self, url: String, secret: String) {
        let mut guard = self.endpoint.write().await;
        guard.url = url;
        guard.secret = secret;
    }

    pub async fn endpoint(&self) -> (String, String) {
        let guard = self.endpoint.read().await;
        (guard.url.clone(), guard.secret.clone())
    }

    async fn call<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        mut params: Vec<Value>,
    ) -> Result<T, EngineError> {
        let (url, secret) = self.endpoint().await;
        let mut full_params = Vec::with_capacity(params.len() + 1);
        full_params.push(Value::String(format!("token:{secret}")));
        full_params.append(&mut params);

        let id = self.request_id.fetch_add(1, Ordering::Relaxed);
        let body = json!({
            "jsonrpc": "2.0",
            "id": id.to_string(),
            "method": method,
            "params": full_params,
        });

        let response = self.http.post(&url).json(&body).send().await?;
        let envelope: RpcEnvelope<T> = response.json().await?;

        if let Some(err) = envelope.error {
            return Err(EngineError::Rpc {
                code: err.code,
                message: err.message,
            });
        }

        envelope.result.ok_or(EngineError::MissingResult)
    }

    pub async fn get_version(&self) -> Result<Aria2Version, EngineError> {
        self.call("aria2.getVersion", vec![]).await
    }

    pub async fn add_uri(
        &self,
        uris: Vec<String>,
        options: Option<DownloadOptions>,
    ) -> Result<String, EngineError> {
        let mut params = vec![Value::Array(uris.into_iter().map(Value::String).collect())];
        if let Some(opts) = options {
            params.push(Value::Object(opts.to_rpc_map()));
        }
        self.call("aria2.addUri", params).await
    }

    pub async fn add_torrent(
        &self,
        torrent_base64: String,
        options: Option<DownloadOptions>,
    ) -> Result<String, EngineError> {
        let mut params = vec![Value::String(torrent_base64), Value::Array(vec![])];
        if let Some(opts) = options {
            params.push(Value::Object(opts.to_rpc_map()));
        }
        self.call("aria2.addTorrent", params).await
    }

    pub async fn pause(&self, gid: String) -> Result<String, EngineError> {
        self.call("aria2.pause", vec![Value::String(gid)]).await
    }

    pub async fn force_pause(&self, gid: String) -> Result<String, EngineError> {
        self.call("aria2.forcePause", vec![Value::String(gid)]).await
    }

    pub async fn unpause(&self, gid: String) -> Result<String, EngineError> {
        self.call("aria2.unpause", vec![Value::String(gid)]).await
    }

    pub async fn remove(&self, gid: String) -> Result<String, EngineError> {
        self.call("aria2.remove", vec![Value::String(gid)]).await
    }

    pub async fn force_remove(&self, gid: String) -> Result<String, EngineError> {
        self.call("aria2.forceRemove", vec![Value::String(gid)]).await
    }

    pub async fn remove_download_result(&self, gid: String) -> Result<String, EngineError> {
        self.call("aria2.removeDownloadResult", vec![Value::String(gid)])
            .await
    }

    pub async fn tell_status(&self, gid: String) -> Result<DownloadTask, EngineError> {
        let raw: RawDownloadTask = self
            .call("aria2.tellStatus", vec![Value::String(gid)])
            .await?;
        Ok(DownloadTask::from(raw))
    }

    pub async fn tell_active(&self) -> Result<Vec<DownloadTask>, EngineError> {
        let raw: Vec<RawDownloadTask> = self.call("aria2.tellActive", vec![]).await?;
        Ok(raw.into_iter().map(DownloadTask::from).collect())
    }

    pub async fn tell_waiting(&self, offset: i64, num: i64) -> Result<Vec<DownloadTask>, EngineError> {
        let raw: Vec<RawDownloadTask> = self
            .call("aria2.tellWaiting", vec![json!(offset), json!(num)])
            .await?;
        Ok(raw.into_iter().map(DownloadTask::from).collect())
    }

    pub async fn tell_stopped(&self, offset: i64, num: i64) -> Result<Vec<DownloadTask>, EngineError> {
        let raw: Vec<RawDownloadTask> = self
            .call("aria2.tellStopped", vec![json!(offset), json!(num)])
            .await?;
        Ok(raw.into_iter().map(DownloadTask::from).collect())
    }

    pub async fn get_global_stat(&self) -> Result<GlobalStat, EngineError> {
        let raw: RawGlobalStat = self.call("aria2.getGlobalStat", vec![]).await?;
        Ok(GlobalStat::from(raw))
    }

    pub async fn change_global_option(
        &self,
        options: HashMap<String, String>,
    ) -> Result<String, EngineError> {
        let map: serde_json::Map<String, Value> = options
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect();
        self.call("aria2.changeGlobalOption", vec![Value::Object(map)])
            .await
    }

    pub async fn save_session(&self) -> Result<String, EngineError> {
        self.call("aria2.saveSession", vec![]).await
    }

    pub async fn shutdown(&self) -> Result<String, EngineError> {
        self.call("aria2.shutdown", vec![]).await
    }
}

fn parse_u64(s: &str) -> u64 {
    s.trim().parse::<u64>().unwrap_or(0)
}

impl From<RawTaskFile> for TaskFile {
    fn from(raw: RawTaskFile) -> Self {
        Self {
            index: raw.index,
            path: raw.path,
            length: parse_u64(&raw.length),
            completed_length: parse_u64(&raw.completed_length),
            selected: raw.selected.eq_ignore_ascii_case("true"),
        }
    }
}

impl From<RawDownloadTask> for DownloadTask {
    fn from(raw: RawDownloadTask) -> Self {
        let total_length = parse_u64(&raw.total_length);
        let completed_length = parse_u64(&raw.completed_length);
        let download_speed = parse_u64(&raw.download_speed);

        let progress_percent = if total_length > 0 {
            ((completed_length as f64 / total_length as f64) * 100.0).min(100.0)
        } else {
            0.0
        };

        let eta_seconds = if download_speed > 0 && total_length > completed_length {
            Some((total_length - completed_length) / download_speed)
        } else {
            None
        };

        Self {
            gid: raw.gid,
            status: raw.status,
            total_length,
            completed_length,
            upload_length: parse_u64(&raw.upload_length),
            download_speed,
            upload_speed: parse_u64(&raw.upload_speed),
            connections: parse_u64(&raw.connections),
            progress_percent,
            eta_seconds,
            dir: raw.dir,
            error_code: raw.error_code,
            error_message: raw.error_message,
            files: raw.files.into_iter().map(TaskFile::from).collect(),
        }
    }
}

impl From<RawGlobalStat> for GlobalStat {
    fn from(raw: RawGlobalStat) -> Self {
        Self {
            download_speed: parse_u64(&raw.download_speed),
            upload_speed: parse_u64(&raw.upload_speed),
            num_active: parse_u64(&raw.num_active),
            num_waiting: parse_u64(&raw.num_waiting),
            num_stopped: parse_u64(&raw.num_stopped),
            num_stopped_total: parse_u64(&raw.num_stopped_total),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_download_task_and_computes_progress_and_eta() {
        let raw: RawDownloadTask = serde_json::from_value(json!({
            "gid": "2089b05ecca3d829",
            "status": "active",
            "totalLength": "10000",
            "completedLength": "2500",
            "uploadLength": "0",
            "downloadSpeed": "500",
            "uploadSpeed": "0",
            "connections": "8",
            "dir": "/downloads",
            "files": [{
                "index": "1",
                "path": "/downloads/ubuntu.iso",
                "length": "10000",
                "completedLength": "2500",
                "selected": "true"
            }]
        }))
        .expect("valid json");

        let task = DownloadTask::from(raw);
        assert_eq!(task.total_length, 10_000);
        assert_eq!(task.completed_length, 2_500);
        assert_eq!(task.download_speed, 500);
        assert!((task.progress_percent - 25.0).abs() < f64::EPSILON);
        assert_eq!(task.eta_seconds, Some(15));
        assert!(task.files[0].selected);
    }

    #[test]
    fn download_options_serializes_aria2_keys() {
        let opts = DownloadOptions {
            dir: Some("/tmp/dl".into()),
            out: Some("file.zip".into()),
            header: Some(vec!["Referer: https://example.com".into()]),
            split: Some(16),
            max_connection_per_server: Some(16),
            ..Default::default()
        };
        let map = opts.to_rpc_map();
        assert_eq!(map.get("dir").and_then(|v| v.as_str()), Some("/tmp/dl"));
        assert_eq!(map.get("split").and_then(|v| v.as_str()), Some("16"));
        assert_eq!(
            map.get("max-connection-per-server")
                .and_then(|v| v.as_str()),
            Some("16")
        );
    }
}
