//! Retrieving one snapshot, under the three bounds this adapter declares.
//!
//! The bounds are a timeout, a maximum size and a content type that must be an
//! image, and each has a [`VisionError`] variant of its own so that a caller is
//! told which bound refused rather than that something went wrong. A host that
//! is not reachable at all is the fourth answer, and it is the one no served
//! response can produce.

use printobserver_types::RawBytes;
use printobserver_vision_api::{FetchedImage, VisionError};
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, Response};

/// The content-type prefix this adapter accepts, and it accepts no other.
const IMAGE_PREFIX: &str = "image/";

/// Whether this many bytes is within the limit.
///
/// The count arrives as whatever the source counted in — the declared length in
/// `u64`, what has been read in `usize` — and the limit is the `i64` the port's
/// [`VisionError::TooLarge`] names, so a count that does not fit an `i64` is
/// over the limit rather than something to cast.
fn within(byte_count: impl TryInto<i64>, max_bytes: i64) -> bool {
    byte_count
        .try_into()
        .is_ok_and(|count: i64| count <= max_bytes)
}

/// Which refusal one transport failure is.
///
/// A timeout is its own answer; everything else reaching the host failed is
/// unreachable, carrying the transport's own words for what went wrong.
fn refusal_for(error: &reqwest::Error) -> VisionError {
    if error.is_timeout() {
        VisionError::TimedOut
    } else {
        VisionError::Unreachable {
            detail: error.to_string(),
        }
    }
}

/// The content type a response was served as, without its parameters.
fn essence_of(response: &Response) -> Option<String> {
    let declared = response.headers().get(CONTENT_TYPE)?.to_str().ok()?;
    Some(declared.to_owned())
}

/// Read a response's body, refusing one that grows past the limit.
async fn body_within(mut response: Response, max_bytes: i64) -> Result<RawBytes, VisionError> {
    if response
        .content_length()
        .is_some_and(|declared| !within(declared, max_bytes))
    {
        return Err(VisionError::TooLarge { limit: max_bytes });
    }
    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| refusal_for(&error))?
    {
        bytes.extend_from_slice(&chunk);
        if !within(bytes.len(), max_bytes) {
            return Err(VisionError::TooLarge { limit: max_bytes });
        }
    }
    Ok(RawBytes::new(bytes))
}

/// Retrieve the image one URL names, under the bounds given.
pub(crate) async fn image(
    client: &Client,
    source_url: &str,
    max_bytes: i64,
) -> Result<FetchedImage, VisionError> {
    let response = client
        .get(source_url)
        .send()
        .await
        .map_err(|error| refusal_for(&error))?;
    let status = response.status();
    if !status.is_success() {
        return Err(VisionError::Unreachable {
            detail: format!("{source_url} answered {status}"),
        });
    }
    let content_type =
        essence_of(&response).ok_or_else(|| VisionError::UnacceptableContentType {
            content_type: String::new(),
        })?;
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if !essence.starts_with(IMAGE_PREFIX) {
        return Err(VisionError::UnacceptableContentType { content_type });
    }
    Ok(FetchedImage {
        bytes: body_within(response, max_bytes).await?,
        content_type,
    })
}

#[cfg(test)]
mod tests {
    use super::within;

    /// A count that does not fit the limit's own type is over the limit.
    #[test]
    fn a_count_that_does_not_fit_is_over_the_limit() {
        assert!(within(4_u64, 8));
        assert!(within(8_usize, 8));
        assert!(!within(9_u64, 8));
        assert!(!within(u64::MAX, i64::MAX));
    }
}
