use tauri_plugin_http::reqwest::Client as ReqwestClient;

mod auth;
mod builder;
mod cookies;
mod error;
mod headers;
mod inspect;
mod proxy;
mod stream;

#[cfg(test)]
mod tests;

pub use auth::AuthType;
pub use error::*;
pub use inspect::sanitize_file_name;
pub use proxy::*;

/// HTTP client for one download. Cheap to clone: the inner reqwest client is reference
/// counted and shares its connection pool / TLS sessions between all chunks.
#[derive(Clone)]
pub struct Client {
    url: String,
    client: ReqwestClient,
    auth: Option<AuthType>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("url", &self.url).finish()
    }
}
