use std::sync::{atomic::Ordering, Arc};
use std::time::Duration;

use futures_util::StreamExt;
use tauri_plugin_http::reqwest::Url;
use tokio::time::timeout;

use crate::{
    client::{Client, ClientError},
    dispatch,
    file::FileWriterHandle,
    registry::Report,
};

#[derive(Debug, PartialEq, Eq)]
pub enum HlsPlaylist {
    Master(Vec<HlsVariant>),
    Media {
        init_segment: Option<String>,
        segments: Vec<String>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct HlsVariant {
    pub bandwidth: u64,
    pub url: String,
}

pub fn is_hls_source(url: &str, content_type: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    if ct.contains("mpegurl") || ct.contains("dash+xml") {
        return true;
    }
    Url::parse(url)
        .ok()
        .map(|u| {
            let p = u.path().to_ascii_lowercase();
            p.ends_with(".m3u8") || p.ends_with(".mpd")
        })
        .unwrap_or_else(|| {
            let lower = url.to_ascii_lowercase();
            lower.contains(".m3u8") || lower.contains(".mpd")
        })
}

/// Parses a DASH (`.mpd`) XML manifest and extracts initialization + segment URLs
/// (supporting `<BaseURL>`, `<Initialization sourceURL="...">`, and `<SegmentURL media="...">`).
pub fn parse_dash_manifest(xml: &str, base_url: &str) -> Option<Vec<String>> {
    if !xml.contains("<MPD") {
        return None;
    }
    let mut base = Url::parse(base_url).ok()?;
    if let (Some(s), Some(e)) = (xml.find("<BaseURL>"), xml.find("</BaseURL>")) {
        let raw_base = xml[s + "<BaseURL>".len()..e].trim();
        if let Ok(joined) = base.join(raw_base) {
            base = joined;
        }
    }

    let mut urls = Vec::new();
    for attr in ["sourceURL=\"", "media=\""] {
        let mut cursor = xml;
        while let Some(idx) = cursor.find(attr) {
            let rest = &cursor[idx + attr.len()..];
            if let Some(end) = rest.find('"') {
                let candidate = rest[..end].trim();
                if !candidate.is_empty() && !candidate.contains('$') {
                    if let Ok(resolved) = base.join(candidate) {
                        let s = resolved.to_string();
                        if !urls.contains(&s) {
                            urls.push(s);
                        }
                    }
                }
                cursor = &rest[end + 1..];
            } else {
                break;
            }
        }
    }

    (!urls.is_empty()).then_some(urls)
}

pub fn parse_hls_playlist(text: &str, base_url: &str) -> Option<HlsPlaylist> {
    let base = Url::parse(base_url).ok()?;
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());

    let first = lines.next()?;
    if !first.starts_with("#EXTM3U") {
        return None;
    }

    let mut variants = Vec::new();
    let mut segments = Vec::new();
    let mut init_segment: Option<String> = None;
    let mut pending_bandwidth: Option<u64> = None;

    for line in lines {
        if let Some(rest) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let bw = rest
                .split(',')
                .find_map(|attr| attr.trim().strip_prefix("BANDWIDTH="))
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(0);
            pending_bandwidth = Some(bw);
            continue;
        }

        if let Some(rest) = line.strip_prefix("#EXT-X-MAP:") {
            if let Some(uri) = extract_quoted_attr(rest, "URI") {
                if let Ok(resolved) = base.join(&uri) {
                    init_segment = Some(resolved.to_string());
                }
            }
            continue;
        }

        if line.starts_with('#') {
            continue;
        }

        let resolved = base.join(line).ok()?.to_string();
        if let Some(bandwidth) = pending_bandwidth.take() {
            variants.push(HlsVariant {
                bandwidth,
                url: resolved,
            });
        } else {
            segments.push(resolved);
        }
    }

    if !variants.is_empty() {
        variants.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));
        Some(HlsPlaylist::Master(variants))
    } else {
        Some(HlsPlaylist::Media {
            init_segment,
            segments,
        })
    }
}

fn extract_quoted_attr(attrs: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    for part in attrs.split(',') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix(&prefix) {
            return Some(val.trim_matches('"').to_string());
        }
    }
    None
}

async fn fetch_bytes_with_url(
    base_client: &Client,
    target_url: &str,
    auth: &Option<crate::client::AuthType>,
    proxy: &Option<crate::client::ProxyType>,
    headers: &Option<std::collections::HashMap<String, String>>,
    cookies: &Option<std::collections::HashMap<String, String>>,
    timeout_secs: u64,
) -> Result<Vec<u8>, ClientError> {
    let client = if target_url == base_client_url(base_client) {
        base_client.clone()
    } else {
        Client::new(target_url, auth, proxy, headers, cookies)?
    };

    let mut stream = client.stream(None).await?;
    let mut out = Vec::new();
    while let Some(item) = timeout(Duration::from_secs(timeout_secs), stream.next())
        .await
        .map_err(|_| ClientError::StreamTimeout)?
    {
        out.extend_from_slice(&item?);
    }
    Ok(out)
}

fn base_client_url(_c: &Client) -> &str {
    ""
}

impl super::DownloadWorker {
    pub(super) async fn download_hls_stream(
        self: &Arc<Self>,
        chunk_index: i64,
    ) -> Result<(), ClientError> {
        let (url, auth, proxy, headers, cookies, timeout_secs, file) = {
            let w = self.data.read().await;
            (
                w.download.url.clone(),
                w.download.auth.clone(),
                w.download.proxy.clone(),
                w.download.headers.clone(),
                w.download.cookies.clone(),
                w.download.timeout_secs.max(10) as u64,
                w.file.clone(),
            )
        };

        let raw = fetch_bytes_with_url(
            &self.client,
            &url,
            &auth,
            &proxy,
            &headers,
            &cookies,
            timeout_secs,
        )
        .await?;
        let text = String::from_utf8_lossy(&raw).into_owned();

        if let Some(dash_urls) = parse_dash_manifest(&text, &url) {
            return write_hls_segments(
                self,
                chunk_index,
                &dash_urls,
                &auth,
                &proxy,
                &headers,
                &cookies,
                timeout_secs,
                &file,
                &self.report,
            )
            .await;
        }

        let (media_url, media_text) = match parse_hls_playlist(&text, &url) {
            Some(HlsPlaylist::Master(variants)) => {
                let best = variants
                    .into_iter()
                    .next()
                    .ok_or(ClientError::MissingContentLength)?;
                let sub_raw = fetch_bytes_with_url(
                    &self.client,
                    &best.url,
                    &auth,
                    &proxy,
                    &headers,
                    &cookies,
                    timeout_secs,
                )
                .await?;
                (best.url, String::from_utf8_lossy(&sub_raw).into_owned())
            }
            Some(HlsPlaylist::Media { .. }) => (url.clone(), text),
            None => return Err(ClientError::MissingContentLength),
        };

        let (init_segment, segments) = match parse_hls_playlist(&media_text, &media_url) {
            Some(HlsPlaylist::Media {
                init_segment,
                segments,
            }) if !segments.is_empty() => (init_segment, segments),
            _ => return Err(ClientError::MissingContentLength),
        };

        let mut all_urls = Vec::with_capacity(segments.len() + 1);
        if let Some(init) = init_segment {
            all_urls.push(init);
        }
        all_urls.extend(segments);

        write_hls_segments(
            self,
            chunk_index,
            &all_urls,
            &auth,
            &proxy,
            &headers,
            &cookies,
            timeout_secs,
            &file,
            &self.report,
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn write_hls_segments(
    worker: &Arc<super::DownloadWorker>,
    chunk_index: i64,
    urls: &[String],
    auth: &Option<crate::client::AuthType>,
    proxy: &Option<crate::client::ProxyType>,
    headers: &Option<std::collections::HashMap<String, String>>,
    cookies: &Option<std::collections::HashMap<String, String>>,
    timeout_secs: u64,
    file: &FileWriterHandle,
    report: &Arc<Report>,
) -> Result<(), ClientError> {
    let mut offset = report
        .chunks_wrote_bytes
        .get(&chunk_index)
        .map(|b| b.load(Ordering::SeqCst))
        .unwrap_or(0);

    // Reset on retry of HLS stream so segments don't duplicate.
    if offset > 0 {
        offset = 0;
        if let Some(counter) = report.chunks_wrote_bytes.get(&chunk_index) {
            counter.store(0, Ordering::SeqCst);
        }
        report.total_downloaded_bytes.store(0, Ordering::SeqCst);
        report.total_wrote_bytes.store(0, Ordering::SeqCst);
    }

    for seg_url in urls {
        let seg_client = Client::new(seg_url, auth, proxy, headers, cookies)?;
        let mut stream = seg_client.stream(None).await?;

        while let Some(item) = timeout(Duration::from_secs(timeout_secs), stream.next())
            .await
            .map_err(|_| ClientError::StreamTimeout)?
        {
            let bytes = item?;
            let len = bytes.len() as u64;
            if len == 0 {
                continue;
            }

            file.write(chunk_index, offset, bytes.to_vec())
                .await
                .map_err(ClientError::DiskWrite)?;

            worker.limiter(len).await;
            offset += len;

            let _ = dispatch!(registry, UpdateNetworkReport, (worker.download_id, len));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_master_playlist_and_picks_highest_bandwidth() {
        let m3u8 = r#"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360
low/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2800000,RESOLUTION=1920x1080
high/index.m3u8
"#;
        let parsed = parse_hls_playlist(m3u8, "https://cdn.example.com/live/master.m3u8").unwrap();
        match parsed {
            HlsPlaylist::Master(v) => {
                assert_eq!(v.len(), 2);
                assert_eq!(v[0].bandwidth, 2_800_000);
                assert_eq!(v[0].url, "https://cdn.example.com/live/high/index.m3u8");
            }
            _ => panic!("expected master playlist"),
        }
    }

    #[test]
    fn parses_media_playlist_with_init_map() {
        let m3u8 = r#"#EXTM3U
#EXT-X-VERSION:7
#EXT-X-MAP:URI="init.mp4"
#EXTINF:4.000,
seg-001.m4s
#EXTINF:4.000,
seg-002.m4s
#EXT-X-ENDLIST
"#;
        let parsed =
            parse_hls_playlist(m3u8, "https://cdn.example.com/video/720p/playlist.m3u8").unwrap();
        assert_eq!(
            parsed,
            HlsPlaylist::Media {
                init_segment: Some("https://cdn.example.com/video/720p/init.mp4".into()),
                segments: vec![
                    "https://cdn.example.com/video/720p/seg-001.m4s".into(),
                    "https://cdn.example.com/video/720p/seg-002.m4s".into(),
                ],
            }
        );
    }

    #[test]
    fn parses_dash_mpd_manifest() {
        let mpd = r#"<?xml version="1.0"?>
<MPD>
  <BaseURL>https://cdn.example.com/dash/</BaseURL>
  <SegmentList>
    <Initialization sourceURL="init.mp4"/>
    <SegmentURL media="seg1.m4s"/>
    <SegmentURL media="seg2.m4s"/>
  </SegmentList>
</MPD>"#;
        let urls = parse_dash_manifest(mpd, "https://cdn.example.com/stream.mpd").unwrap();
        assert_eq!(
            urls,
            vec![
                "https://cdn.example.com/dash/init.mp4",
                "https://cdn.example.com/dash/seg1.m4s",
                "https://cdn.example.com/dash/seg2.m4s",
            ]
        );
    }
}
