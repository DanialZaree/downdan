pub mod rpc;
pub mod sidecar;

pub use rpc::{Aria2Version, DownloadOptions, DownloadTask, EngineError, GlobalStat};
pub use sidecar::{spawn_sync_loop, EngineState};
