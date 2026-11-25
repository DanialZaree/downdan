use std::{collections::HashMap, time::Duration};
use tauri_plugin_http::reqwest::Client as ReqwestClient;

use crate::{client::ProxyType, emitter::Emitter};

const DEFAULT_USER_AGENT: &str = concat!("danDown/", env!("CARGO_PKG_VERSION"));

impl super::Client {
    pub fn new(
        url: &str,
        auth: &Option<super::AuthType>,
        proxy: &Option<super::ProxyType>,
        headers: &Option<HashMap<String, String>>,
        cookies: &Option<HashMap<String, String>>,
    ) -> Result<Self, super::ClientError> {
        let mut builder = ReqwestClient::builder()
            .connect_timeout(Duration::from_secs(10))
            .tcp_keepalive(Duration::from_secs(30))
            // Many servers reject requests without a User-Agent; custom headers below override it.
            .user_agent(DEFAULT_USER_AGENT);

        match headers {
            Some(custom_headers) => {
                builder = builder.default_headers(Self::get_headers(custom_headers))
            }
            None => {}
        }

        match cookies {
            Some(custom_cookies) => {
                let jar = Self::get_cookie_jar(url, custom_cookies.to_owned());
                builder = builder.cookie_provider(jar);
            }
            None => {}
        };

        match proxy {
            Some(ProxyType::System) => {}
            Some(custom_proxy) => match Self::get_proxy(custom_proxy) {
                Ok(p) => builder = builder.proxy(p),
                Err(e) => Emitter::emit_error(e.to_string()),
            },
            None => builder = builder.no_proxy(),
        };

        let client = builder.build()?;

        Ok(Self {
            client,
            url: url.to_string(),
            auth: auth.clone(),
        })
    }
}
