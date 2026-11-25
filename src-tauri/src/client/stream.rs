use std::pin::Pin;

use futures_util::{Stream, StreamExt};
use tauri::http::{
    header::{CONTENT_RANGE, RANGE},
    Method, StatusCode,
};
use tokio_util::bytes::Bytes;

use super::inspect::parse_content_range;

pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, super::ClientError>> + Send>>;

impl super::Client {
    /// Opens a GET stream, optionally for an inclusive byte range, and validates that the
    /// server actually honoured the request before any byte reaches the file.
    pub async fn stream(&self, range: Option<(i64, i64)>) -> Result<ByteStream, super::ClientError> {
        let request = self.client.request(Method::GET, &self.url);
        let request = Self::auth_handler(request, &self.auth);

        let request = match range {
            Some((start, end)) => request.header(RANGE, format!("bytes={}-{}", start, end)),
            None => request,
        };

        let response = request.send().await?;

        let content_range = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());

        validate_stream_response(response.status(), range, content_range.as_deref())?;

        let stream = response
            .bytes_stream()
            .map(|res| res.map_err(super::ClientError::from));

        Ok(Box::pin(stream))
    }
}

/// Decides whether a GET response is safe to write into the file at the requested offset.
///
/// - Error statuses (403/429/5xx…) are surfaced as `Http` so retry policy can decide.
/// - A ranged request must come back as `206` starting at the requested byte. A `200` is
///   only acceptable when the range starts at 0 (the full body begins at the right offset
///   and the worker truncates at the chunk end); otherwise writing it would corrupt the file.
pub fn validate_stream_response(
    status: StatusCode,
    range: Option<(i64, i64)>,
    content_range: Option<&str>,
) -> Result<(), super::ClientError> {
    if !status.is_success() {
        return Err(super::ClientError::Http { status });
    }

    let Some((start, _end)) = range else {
        return Ok(());
    };

    match status {
        StatusCode::PARTIAL_CONTENT => match content_range.and_then(parse_content_range) {
            Some(cr) if cr.start == start as u64 => Ok(()),
            Some(_) => Err(super::ClientError::UnexpectedContentRange),
            // Some servers omit Content-Range on 206; trust the status.
            None => Ok(()),
        },
        StatusCode::OK if start == 0 => Ok(()),
        StatusCode::OK => Err(super::ClientError::RangeNotSupported),
        status => Err(super::ClientError::Http { status }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ClientError;

    #[test]
    fn rejects_error_statuses() {
        let err = validate_stream_response(StatusCode::FORBIDDEN, Some((0, 10)), None).unwrap_err();
        assert!(matches!(err, ClientError::Http { status } if status == StatusCode::FORBIDDEN));
    }

    #[test]
    fn accepts_matching_partial_content() {
        assert!(validate_stream_response(
            StatusCode::PARTIAL_CONTENT,
            Some((100, 199)),
            Some("bytes 100-199/1000")
        )
        .is_ok());
    }

    #[test]
    fn rejects_mismatched_partial_content() {
        let err = validate_stream_response(
            StatusCode::PARTIAL_CONTENT,
            Some((100, 199)),
            Some("bytes 0-99/1000"),
        )
        .unwrap_err();
        assert!(matches!(err, ClientError::UnexpectedContentRange));
    }

    #[test]
    fn full_body_only_allowed_from_offset_zero() {
        assert!(validate_stream_response(StatusCode::OK, Some((0, 499)), None).is_ok());
        let err = validate_stream_response(StatusCode::OK, Some((500, 999)), None).unwrap_err();
        assert!(matches!(err, ClientError::RangeNotSupported));
    }

    #[test]
    fn unranged_requests_accept_any_success() {
        assert!(validate_stream_response(StatusCode::OK, None, None).is_ok());
    }
}
