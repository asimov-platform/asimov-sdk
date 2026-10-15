// This is free and unencumbered software released into the public domain.

//! RFC 9530 fields for complete, buffered HTTP request representations.

use alloc::format;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::header::{HeaderMap, HeaderValue};
use sha2::{Digest as _, Sha256};

/// A relative SHA-256 preference for a `Want-*-Digest` field.
///
/// RFC 9530 requires an integer in `0..=10`: zero means unacceptable, one is
/// least preferred, and ten is most preferred. This is not an HTTP `q` value.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DigestPreference(u8);

impl DigestPreference {
    /// Creates a preference, or returns `None` for a value greater than ten.
    pub const fn new(value: u8) -> Option<Self> {
        if value <= 10 { Some(Self(value)) } else { None }
    }

    /// Returns the preference's integer value, in `0..=10`.
    pub const fn value(self) -> u8 {
        self.0
    }
}

/// Outbound RFC 9530 digest and preference fields for [`super::Executor`].
///
/// By default, requests carry a SHA-256 `Content-Digest` and no other digest
/// fields. Each enabled field is a Structured Fields dictionary with the single
/// key `sha-256`. SHA-256 is the only supported algorithm.
///
/// The HTTP adapter sends complete JSON representations without content coding,
/// so content and representation digests hash the same bytes. `Repr-Digest`
/// describes the enclosed request, not the resource fetched or listed by it.
/// Neither digest covers HTTP framing, headers, or the response body. Digests
/// alone do not authenticate a request.
///
/// Preference fields are advisory: servers may ignore them. They do not enable
/// response-digest verification or automatic negotiation/retries in this client.
///
/// # Example
///
/// ```
/// use asimov_remote::http::{DigestOptions, DigestPreference};
///
/// let options = DigestOptions {
///     representation: true,
///     want_content: DigestPreference::new(10),
///     ..DigestOptions::default()
/// };
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DigestOptions {
    /// Sends `Content-Digest` when true. Defaults to true.
    pub content: bool,
    /// Sends `Repr-Digest` when true. Defaults to false.
    pub representation: bool,
    /// Sends `Want-Content-Digest` when set. Defaults to `None`.
    pub want_content: Option<DigestPreference>,
    /// Sends `Want-Repr-Digest` when set. Defaults to `None`.
    pub want_representation: Option<DigestPreference>,
}

impl Default for DigestOptions {
    fn default() -> Self {
        Self {
            content: true,
            representation: false,
            want_content: None,
            want_representation: None,
        }
    }
}

impl DigestOptions {
    /// The input must be the complete representation as sent, after any content
    /// coding but before transfer coding. This is not a partial-body helper.
    pub(super) fn headers(&self, body: &[u8]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if self.content || self.representation {
            // Hash once for both fields. Structured Fields byte sequences use
            // standard, padded Base64 of the raw hash, not hex or Base64url.
            let digest = STANDARD.encode(Sha256::digest(body));
            let value = HeaderValue::try_from(format!("sha-256=:{digest}:"))
                .expect("digest dictionaries contain only visible ASCII");
            if self.content {
                headers.insert("content-digest", value.clone());
            }
            if self.representation {
                headers.insert("repr-digest", value);
            }
        }
        for (name, preference) in [
            ("want-content-digest", self.want_content),
            ("want-repr-digest", self.want_representation),
        ] {
            if let Some(preference) = preference {
                headers.insert(
                    name,
                    HeaderValue::try_from(format!("sha-256={}", preference.value()))
                        .expect("digest preferences contain only visible ASCII"),
                );
            }
        }
        headers
    }
}
