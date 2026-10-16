// This is free and unencumbered software released into the public domain.

use alloc::{format, string::String};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest as _, Sha256};

/// A SHA-256 digest of content bytes, after content coding and before framing.
///
/// This value can also describe representation data in `Repr-Digest`, provided
/// the caller hashes the entire selected representation, not a range or an
/// empty HEAD response. JSON canonicalization is not part of RFC 9530 hashing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentDigest([u8; 32]);

impl ContentDigest {
    /// Hashes the exact bytes to be transmitted.
    pub fn new(content: &[u8]) -> Self {
        Self(Sha256::digest(content).into())
    }

    /// Wraps a previously computed SHA-256 hash.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the raw SHA-256 hash.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Serializes the RFC 9530 dictionary using padded standard Base64.
    pub fn to_field_value(self) -> String {
        format!("sha-256=:{}:", STANDARD.encode(self.0))
    }

    /// Parses a digest dictionary and selects its SHA-256 member.
    ///
    /// Unknown algorithms are ignored after validating the dictionary's syntax
    /// and byte-sequence member types. No weaker algorithm is used as fallback.
    #[cfg(feature = "std")]
    pub fn parse(value: &str) -> Result<Self, crate::Error> {
        Self::find(value)?.ok_or(crate::Error::ContentDigest)
    }

    #[cfg(feature = "std")]
    fn find(value: &str) -> Result<Option<Self>, crate::Error> {
        let dictionary = crate::fields::dictionary(value)?;
        for entry in dictionary.values() {
            if !matches!(entry, sfv::ListEntry::Item(item)
                if item.bare_item.as_byte_sequence().is_some())
            {
                return Err(crate::Error::InvalidField(
                    "digest members must be byte sequences",
                ));
            }
        }
        let Some(sfv::ListEntry::Item(item)) = dictionary.get("sha-256") else {
            return Ok(None);
        };
        let bytes = item
            .bare_item
            .as_byte_sequence()
            .ok_or(crate::Error::ContentDigest)?;
        Ok(Some(Self(
            bytes.try_into().map_err(|_| crate::Error::ContentDigest)?,
        )))
    }

    /// Verifies every supplied SHA-256 digest, preserving header/trailer scope.
    /// At least one section must contain a SHA-256 digest.
    #[cfg(feature = "std")]
    pub fn verify(
        self,
        headers: &http::HeaderMap,
        trailers: &http::HeaderMap,
    ) -> Result<(), crate::Error> {
        let mut found = false;
        for section in [headers, trailers] {
            if let Some(value) = crate::fields::combined(&[section], "content-digest")?
                && let Some(expected) = Self::find(&value)?
            {
                if expected != self {
                    return Err(crate::Error::ContentDigest);
                }
                found = true;
            }
        }
        if found {
            Ok(())
        } else {
            Err(crate::Error::ContentDigest)
        }
    }
}

/// A SHA-256 preference weight for `Want-Content-Digest` or `Want-Repr-Digest`.
/// Zero means unacceptable, one least preferred, and ten most preferred.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DigestPreference(u8);

impl DigestPreference {
    /// Returns `None` for a weight outside RFC 9530's `0..=10` range.
    pub const fn new(value: u8) -> Option<Self> {
        if value <= 10 { Some(Self(value)) } else { None }
    }

    /// Returns the integer preference weight.
    pub const fn value(self) -> u8 {
        self.0
    }
}

/// RFC 9530 output fields for a complete, buffered representation.
///
/// The default sends only a SHA-256 `Content-Digest`. Preferences are advisory
/// and do not activate verification. Representation digests must describe the
/// complete selected representation, which is the body for ordinary JSON POSTs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DigestOptions {
    /// Send `Content-Digest` (default true).
    pub content: bool,
    /// Send `Repr-Digest` (default false).
    pub representation: bool,
    /// Optional `Want-Content-Digest` weight.
    pub want_content: Option<DigestPreference>,
    /// Optional `Want-Repr-Digest` weight.
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
    /// Creates fields for complete representation bytes, usable in either a
    /// header or trailer map. Partial representations require separate hashing.
    #[cfg(feature = "std")]
    pub fn headers(&self, body: &[u8]) -> http::HeaderMap {
        let mut headers = http::HeaderMap::new();
        if self.content || self.representation {
            let value = http::HeaderValue::try_from(ContentDigest::new(body).to_field_value())
                .expect("digest fields contain only visible ASCII");
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
                    http::HeaderValue::try_from(format!("sha-256={}", preference.value()))
                        .expect("digest preferences contain only visible ASCII"),
                );
            }
        }
        headers
    }
}
