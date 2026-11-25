//! Integration tests for the HTTP client against a tiny in-process HTTP/1.1 server.
//! The server can be configured to misbehave the way real CDNs do (reject HEAD,
//! hide Accept-Ranges, ignore Range, omit Content-Length).

use std::sync::Arc;

use futures_util::StreamExt;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use super::{Client, ClientError};

#[derive(Clone, Copy, Default)]
struct ServerBehavior {
    reject_head: bool,
    hide_accept_ranges: bool,
    ignore_range: bool,
    no_content_length: bool,
    status_override: Option<u16>,
}

fn body() -> Arc<Vec<u8>> {
    Arc::new((0..10_000u32).map(|i| (i % 251) as u8).collect())
}

async fn spawn_server(behavior: ServerBehavior, data: Arc<Vec<u8>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break };
            let data = Arc::clone(&data);

            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 1024];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut tmp).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }

                let request = String::from_utf8_lossy(&buf).to_string();
                let method = request.split_whitespace().next().unwrap_or("").to_string();
                let range = request.lines().find_map(|l| {
                    let lower = l.to_ascii_lowercase();
                    lower.strip_prefix("range: bytes=").map(|r| {
                        let (s, e) = r.trim().split_once('-').unwrap();
                        (s.parse::<usize>().unwrap(), e.parse::<usize>().ok())
                    })
                });

                let response = build_response(&method, range, behavior, &data);
                let _ = socket.write_all(&response).await;
                let _ = socket.shutdown().await;
            });
        }
    });

    format!("http://{addr}/files/test%20file.bin")
}

fn build_response(
    method: &str,
    range: Option<(usize, Option<usize>)>,
    b: ServerBehavior,
    data: &[u8],
) -> Vec<u8> {
    if let Some(code) = b.status_override {
        return format!("HTTP/1.1 {code} Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .into_bytes();
    }

    if method == "HEAD" && b.reject_head {
        return b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec();
    }

    let total = data.len();
    let (status, slice, content_range) = match range {
        Some((start, end)) if !b.ignore_range => {
            let end = end.unwrap_or(total - 1).min(total - 1);
            (
                "206 Partial Content",
                &data[start..=end],
                Some(format!("bytes {start}-{end}/{total}")),
            )
        }
        _ => ("200 OK", data, None),
    };

    let mut head = format!("HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n");
    if !b.hide_accept_ranges && !b.ignore_range {
        head.push_str("Accept-Ranges: bytes\r\n");
    }
    if let Some(cr) = content_range {
        head.push_str(&format!("Content-Range: {cr}\r\n"));
    }
    if !b.no_content_length {
        head.push_str(&format!("Content-Length: {}\r\n", slice.len()));
    }
    head.push_str("\r\n");

    let mut out = head.into_bytes();
    if method != "HEAD" {
        out.extend_from_slice(slice);
    }
    out
}

fn client(url: &str) -> Client {
    Client::new(url, &None, &None, &None, &None).unwrap()
}

async fn collect(client: &Client, range: Option<(i64, i64)>) -> Result<Vec<u8>, ClientError> {
    let mut stream = client.stream(range).await?;
    let mut out = Vec::new();
    while let Some(chunk) = stream.next().await {
        out.extend_from_slice(&chunk?);
    }
    Ok(out)
}

#[tokio::test]
async fn inspect_with_well_behaved_server() {
    let url = spawn_server(ServerBehavior::default(), body()).await;
    let info = client(&url).inspect().await.unwrap();

    assert!(info.supports_range);
    assert_eq!(info.content_length, Some(10_000));
    assert_eq!(info.file_name, "test file.bin");
    assert_eq!(info.extension, "bin");
}

#[tokio::test]
async fn inspect_falls_back_to_ranged_get_when_head_is_rejected() {
    let behavior = ServerBehavior { reject_head: true, ..Default::default() };
    let url = spawn_server(behavior, body()).await;
    let info = client(&url).inspect().await.unwrap();

    assert!(info.supports_range);
    assert_eq!(info.content_length, Some(10_000));
}

#[tokio::test]
async fn inspect_detects_range_support_without_accept_ranges_header() {
    let behavior = ServerBehavior { hide_accept_ranges: true, ..Default::default() };
    let url = spawn_server(behavior, body()).await;
    let info = client(&url).inspect().await.unwrap();

    assert!(info.supports_range, "206 response to probe should enable ranges");
    assert_eq!(info.content_length, Some(10_000));
}

#[tokio::test]
async fn inspect_handles_server_without_ranges() {
    let behavior = ServerBehavior { ignore_range: true, ..Default::default() };
    let url = spawn_server(behavior, body()).await;
    let info = client(&url).inspect().await.unwrap();

    assert!(!info.supports_range);
    assert_eq!(info.content_length, Some(10_000));
}

#[tokio::test]
async fn inspect_allows_unknown_size() {
    let behavior = ServerBehavior {
        ignore_range: true,
        no_content_length: true,
        ..Default::default()
    };
    let url = spawn_server(behavior, body()).await;
    let info = client(&url).inspect().await.unwrap();

    assert!(!info.supports_range);
    assert_eq!(info.content_length, None);
}

#[tokio::test]
async fn inspect_reports_http_errors() {
    let behavior = ServerBehavior { status_override: Some(404), ..Default::default() };
    let url = spawn_server(behavior, body()).await;
    let err = client(&url).inspect().await.unwrap_err();

    assert!(matches!(err, ClientError::Http { status } if status.as_u16() == 404));
}

#[tokio::test]
async fn ranged_stream_returns_exact_slice() {
    let data = body();
    let url = spawn_server(ServerBehavior::default(), Arc::clone(&data)).await;
    let bytes = collect(&client(&url), Some((1000, 1999))).await.unwrap();

    assert_eq!(bytes, data[1000..=1999].to_vec());
}

#[tokio::test]
async fn ranged_stream_rejects_server_that_ignores_range() {
    let behavior = ServerBehavior { ignore_range: true, ..Default::default() };
    let url = spawn_server(behavior, body()).await;
    let err = collect(&client(&url), Some((5000, 5999))).await.unwrap_err();

    assert!(matches!(err, ClientError::RangeNotSupported));
}

#[tokio::test]
async fn stream_surfaces_retryable_server_errors() {
    let behavior = ServerBehavior { status_override: Some(503), ..Default::default() };
    let url = spawn_server(behavior, body()).await;
    let err = collect(&client(&url), Some((0, 99))).await.unwrap_err();

    assert!(err.is_retryable());
}
