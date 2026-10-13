// This is free and unencumbered software released into the public domain.

use super::{FetcherRequest, ListerRequest};
use alloc::{format, string::String, vec::Vec};
use core::fmt;
use serde::ser::Error as _;

// Widen usize before comparing so this also compiles on 32-bit targets.
const MAX_SAFE_INTEGER: u128 = (1 << 53) - 1;

#[cfg_attr(docsrs, doc(cfg(feature = "jcs")))]
impl FetcherRequest {
    /// Serializes this request as an RFC 8785 canonical JSON string.
    ///
    /// Requires `jcs` (which enables `serde` and `std`). Uses the request's
    /// Serde representation, sorting object properties recursively and
    /// preserving array order and Unicode strings. No trailing newline is added.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    ///
    /// ```
    /// use asimov_remote::{FetcherOptions, FetcherRequest};
    ///
    /// let request = FetcherRequest::new(
    ///     "example:",
    ///     FetcherOptions::builder().output("jsonl").build(),
    /// );
    /// assert_eq!(
    ///     request.to_canonical_json()?,
    ///     r#"{"options":{"output":"jsonl"},"urls":["example:"]}"#,
    /// );
    /// # Ok::<(), serde_json::Error>(())
    /// ```
    pub fn to_canonical_json(&self) -> Result<String, serde_json::Error> {
        serde_json_canonicalizer::to_string(self)
    }

    /// Serializes this request as RFC 8785 canonical JSON UTF-8 bytes.
    ///
    /// Requires `jcs`. Returns the same encoding as
    /// [`Self::to_canonical_json`], without a trailing newline, for hashing,
    /// signing, or transmission.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn to_canonical_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json_canonicalizer::to_vec(self)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "jcs")))]
impl<T: Clone + fmt::Display, F: fmt::Display> ListerRequest<T, F> {
    /// Serializes this request as an RFC 8785 canonical JSON string.
    ///
    /// Requires `jcs` (which enables `serde` and `std`). Uses the request's
    /// Serde representation, sorting object properties recursively and
    /// preserving array order and Unicode strings. Sort keys and custom output
    /// formats only need [`fmt::Display`]. No trailing newline is added.
    ///
    /// # Errors
    ///
    /// Returns an error if `offset` or `limit` exceeds `2^53 - 1`
    /// (`9007199254740991`), or if serialization fails. This safe-integer limit
    /// prevents loss of count precision in JCS's IEEE 754 number representation.
    ///
    /// ```
    /// use asimov_remote::{ListerOptions, ListerRequest};
    ///
    /// let request: ListerRequest = ListerRequest::new(
    ///     "example:",
    ///     ListerOptions::builder().limit(10).build(),
    /// );
    /// assert_eq!(
    ///     request.to_canonical_json()?,
    ///     r#"{"options":{"limit":10},"url":"example:"}"#,
    /// );
    /// # Ok::<(), serde_json::Error>(())
    /// ```
    pub fn to_canonical_json(&self) -> Result<String, serde_json::Error> {
        self.validate_jcs_counts()?;
        serde_json_canonicalizer::to_string(self)
    }

    /// Serializes this request as RFC 8785 canonical JSON UTF-8 bytes.
    ///
    /// Requires `jcs`. Returns the same encoding as
    /// [`Self::to_canonical_json`], without a trailing newline, for hashing,
    /// signing, or transmission.
    ///
    /// # Errors
    ///
    /// Returns an error if `offset` or `limit` exceeds `2^53 - 1`
    /// (`9007199254740991`), or if serialization fails.
    pub fn to_canonical_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        self.validate_jcs_counts()?;
        serde_json_canonicalizer::to_vec(self)
    }

    fn validate_jcs_counts(&self) -> Result<(), serde_json::Error> {
        for (name, value) in [
            ("offset", self.options.offset),
            ("limit", self.options.limit),
        ] {
            if value.is_some_and(|value| value as u128 > MAX_SAFE_INTEGER) {
                return Err(serde_json::Error::custom(format!(
                    "options.{name} exceeds the JCS safe-integer maximum ({MAX_SAFE_INTEGER})"
                )));
            }
        }
        Ok(())
    }
}
