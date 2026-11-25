use mime2ext::mime2ext;
use tauri::http::{
    header::{ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE},
    HeaderMap, Method, StatusCode,
};
use tauri_plugin_http::reqwest::Url;

#[derive(Clone, Debug)]
pub struct InspectResponse {
    pub supports_range: bool,
    /// `None` when the server does not tell us the size up front (e.g. chunked transfer encoding).
    pub content_length: Option<u64>,
    pub content_type: String,
    pub file_name: String,
    pub extension: String,
    pub url: String,
}

/// What we learned from a single probe request (HEAD or ranged GET).
#[derive(Debug, Default)]
struct ProbeInfo {
    supports_range: bool,
    content_length: Option<u64>,
    headers: HeaderMap,
    final_url: Option<Url>,
}

impl super::Client {
    /// Discovers size, range support and file name for a URL.
    ///
    /// Strategy:
    /// 1. `HEAD` — cheap, but many CDNs reject it (403/405) or omit `Accept-Ranges`.
    /// 2. `GET` with `Range: bytes=0-0` — tells us both range support (206 + `Content-Range`)
    ///    and the total size, without downloading the body.
    pub async fn inspect(&self) -> Result<InspectResponse, super::ClientError> {
        let head = self.probe_head().await;

        let head_is_complete = matches!(
            &head,
            Ok(info) if info.supports_range && info.content_length.is_some()
        );

        let info = if head_is_complete {
            head?
        } else {
            match self.probe_range_get().await {
                Ok(get_info) => merge_probe(head.ok(), get_info),
                // GET probe failed but HEAD gave us something usable: fall back to it.
                Err(get_err) => match head {
                    Ok(head_info) => head_info,
                    Err(_) => return Err(get_err),
                },
            }
        };

        Ok(build_inspect_response(info, &self.url))
    }

    async fn probe_head(&self) -> Result<ProbeInfo, super::ClientError> {
        let request = self.client.request(Method::HEAD, &self.url);
        let request = Self::auth_handler(request, &self.auth);
        let response = request.send().await?;

        let status = response.status();
        if !status.is_success() {
            return Err(super::ClientError::Http { status });
        }

        let headers = response.headers().clone();
        let supports_range = headers
            .get(ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("bytes"))
            .unwrap_or(false);

        Ok(ProbeInfo {
            supports_range,
            content_length: parse_content_length(&headers),
            final_url: Some(response.url().clone()),
            headers,
        })
    }

    async fn probe_range_get(&self) -> Result<ProbeInfo, super::ClientError> {
        let request = self
            .client
            .request(Method::GET, &self.url)
            .header(RANGE, "bytes=0-0");
        let request = Self::auth_handler(request, &self.auth);
        let response = request.send().await?;

        let status = response.status();
        let headers = response.headers().clone();
        let final_url = Some(response.url().clone());

        // Drop the response without reading the body; for a 200 this aborts the transfer.
        drop(response);

        match status {
            StatusCode::PARTIAL_CONTENT => {
                let total = headers
                    .get(CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_content_range)
                    .and_then(|r| r.total);

                Ok(ProbeInfo {
                    supports_range: total.is_some(),
                    content_length: total,
                    headers,
                    final_url,
                })
            }
            s if s.is_success() => Ok(ProbeInfo {
                supports_range: false,
                content_length: parse_content_length(&headers),
                headers,
                final_url,
            }),
            status => Err(super::ClientError::Http { status }),
        }
    }
}

fn merge_probe(head: Option<ProbeInfo>, get: ProbeInfo) -> ProbeInfo {
    let Some(head) = head else { return get };

    let mut headers = head.headers;
    for (name, value) in get.headers.iter() {
        if !headers.contains_key(name) {
            headers.insert(name.clone(), value.clone());
        }
    }

    ProbeInfo {
        supports_range: get.supports_range,
        content_length: get.content_length.or(head.content_length),
        headers,
        final_url: get.final_url.or(head.final_url),
    }
}

fn build_inspect_response(info: ProbeInfo, original_url: &str) -> InspectResponse {
    let final_url = info
        .final_url
        .clone()
        .or_else(|| Url::parse(original_url).ok());

    let content_type = info
        .headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or(v).trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let mime_extension = mime2ext(&content_type).map(|e| e.to_string());

    let file_name = info
        .headers
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_disposition_filename)
        .or_else(|| final_url.as_ref().and_then(file_name_from_url))
        .map(|name| sanitize_file_name(&name))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            let domain = final_url
                .as_ref()
                .and_then(|u| u.domain().map(|d| d.to_string()))
                .unwrap_or_else(|| "file".to_string());
            format!("{domain}.{}", mime_extension.as_deref().unwrap_or("bin"))
        });

    let (file_name, extension) = resolve_extension(file_name, &content_type, mime_extension);
    let final_url_str = final_url
        .map(|u| u.to_string())
        .unwrap_or_else(|| original_url.to_string());

    // HLS (.m3u8) and DASH (.mpd) playlists are downloaded segment-by-segment.
    // Use `.mp4` for fMP4/DASH/SoundCloud streams so media players open them directly.
    if crate::worker::hls::is_hls_source(&final_url_str, &content_type) {
        let stem = std::path::Path::new(&file_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("stream");
        let ext = if final_url_str.to_ascii_lowercase().contains("mp3") {
            "mp3"
        } else if final_url_str.to_ascii_lowercase().contains("opus")
            || final_url_str.to_ascii_lowercase().contains("mp4")
            || final_url_str.to_ascii_lowercase().contains(".mpd")
            || final_url_str.to_ascii_lowercase().contains("sndcdn")
        {
            "mp4"
        } else {
            "ts"
        };
        return InspectResponse {
            supports_range: false,
            content_length: None,
            content_type: "application/vnd.apple.mpegurl".to_string(),
            file_name: format!("{stem}.{ext}"),
            extension: ext.to_string(),
            url: final_url_str,
        };
    }

    InspectResponse {
        supports_range: info.supports_range && info.content_length.is_some(),
        content_length: info.content_length,
        content_type,
        file_name,
        extension,
        url: final_url_str,
    }
}

fn parse_content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
}

/// Prefer the extension in the file name; otherwise derive it from the MIME type
/// and append it so the saved file opens with the right program.
fn resolve_extension(
    file_name: String,
    content_type: &str,
    mime_extension: Option<String>,
) -> (String, String) {
    let existing = std::path::Path::new(&file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());

    match (existing, mime_extension) {
        (Some(ext), _) => (file_name, ext),
        (None, Some(ext)) if content_type != "application/octet-stream" => {
            (format!("{file_name}.{ext}"), ext)
        }
        (None, _) => (file_name, "bin".to_string()),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ContentRange {
    pub start: u64,
    pub end: u64,
    pub total: Option<u64>,
}

/// Parses `Content-Range: bytes <start>-<end>/<total|*>`.
pub fn parse_content_range(value: &str) -> Option<ContentRange> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.trim().split_once('-')?;

    let start = start.trim().parse::<u64>().ok()?;
    let end = end.trim().parse::<u64>().ok()?;
    if end < start {
        return None;
    }

    let total = match total.trim() {
        "*" => None,
        t => Some(t.parse::<u64>().ok()?),
    };

    Some(ContentRange { start, end, total })
}

/// Extracts the file name from a `Content-Disposition` header.
/// Handles RFC 5987 `filename*=UTF-8''...` (preferred) and plain/quoted `filename=`.
pub fn parse_content_disposition_filename(value: &str) -> Option<String> {
    let mut plain: Option<String> = None;
    let mut extended: Option<String> = None;

    for part in split_header_params(value) {
        let Some((key, val)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let val = val.trim();

        if key == "filename*" {
            // charset'language'percent-encoded-value
            let encoded = val.splitn(3, '\'').nth(2).unwrap_or(val);
            let decoded = percent_decode(encoded.trim_matches('"'));
            if !decoded.is_empty() {
                extended = Some(decoded);
            }
        } else if key == "filename" {
            let unquoted = val
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(val)
                .replace("\\\"", "\"");
            if !unquoted.is_empty() {
                plain = Some(unquoted);
            }
        }
    }

    extended.or(plain)
}

/// Splits on `;` while respecting double-quoted sections.
fn split_header_params(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;

    for c in value.chars() {
        match c {
            '\\' if in_quotes && !escaped => {
                escaped = true;
                current.push(c);
                continue;
            }
            '"' if !escaped => in_quotes = !in_quotes,
            ';' if !in_quotes => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        escaped = false;
        current.push(c);
    }
    parts.push(current);
    parts
}

fn file_name_from_url(url: &Url) -> Option<String> {
    url.path_segments()
        .and_then(|segments| segments.filter(|s| !s.is_empty()).last())
        .map(percent_decode)
}

pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
}

/// Removes path components and characters that are invalid on Windows/macOS/Linux,
/// so a malicious `Content-Disposition: filename="../../evil.exe"` cannot escape the
/// download directory.
pub fn sanitize_file_name(name: &str) -> String {
    let base = name.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(name);

    let cleaned: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    let trimmed = cleaned.trim().trim_matches('.').trim().to_string();

    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = trimmed.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return format!("_{trimmed}");
    }

    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_parsing() {
        assert_eq!(
            parse_content_range("bytes 0-0/1234"),
            Some(ContentRange { start: 0, end: 0, total: Some(1234) })
        );
        assert_eq!(
            parse_content_range("bytes 100-199/*"),
            Some(ContentRange { start: 100, end: 199, total: None })
        );
        assert_eq!(parse_content_range("bytes */1234"), None);
        assert_eq!(parse_content_range("bytes 10-5/100"), None);
        assert_eq!(parse_content_range("items 0-1/2"), None);
    }

    #[test]
    fn content_disposition_variants() {
        assert_eq!(
            parse_content_disposition_filename(r#"attachment; filename="report.pdf""#),
            Some("report.pdf".into())
        );
        assert_eq!(
            parse_content_disposition_filename("attachment; filename=plain.zip"),
            Some("plain.zip".into())
        );
        assert_eq!(
            parse_content_disposition_filename(
                r#"attachment; filename="fallback.txt"; filename*=UTF-8''na%C3%AFve%20file.txt"#
            ),
            Some("naïve file.txt".into())
        );
        assert_eq!(
            parse_content_disposition_filename(r#"attachment; filename="a;b.zip""#),
            Some("a;b.zip".into())
        );
        assert_eq!(parse_content_disposition_filename("inline"), None);
    }

    #[test]
    fn sanitizes_dangerous_names() {
        assert_eq!(sanitize_file_name("../../evil.exe"), "evil.exe");
        assert_eq!(sanitize_file_name(r"C:\Windows\system32\x.dll"), "x.dll");
        assert_eq!(sanitize_file_name("what?.txt"), "what_.txt");
        assert_eq!(sanitize_file_name("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_file_name("  .hidden.  "), "hidden");
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("my%20file.zip"), "my file.zip");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("bad%zzvalue"), "bad%zzvalue");
    }

    #[test]
    fn extension_resolution() {
        assert_eq!(
            resolve_extension("video".into(), "video/mp4", Some("mp4".into())),
            ("video.mp4".into(), "mp4".into())
        );
        assert_eq!(
            resolve_extension("archive.ZIP".into(), "application/zip", Some("zip".into())),
            ("archive.ZIP".into(), "zip".into())
        );
        assert_eq!(
            resolve_extension("blob".into(), "application/octet-stream", Some("bin".into())),
            ("blob".into(), "bin".into())
        );
    }
}
