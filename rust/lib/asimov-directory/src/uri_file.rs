// This is free and unencumbered software released into the public domain.

use alloc::string::String;
use core::{fmt, str::FromStr};

/// Owned contents of a `.uri` file: one LF-terminated absolute IRI.
///
/// The `.uri` and `.uris` extensions follow IANA's `text/uri-list` registration
/// ([RFC 2483, Section 5]). This format uses UTF-8 IRIs instead of ASCII URIs
/// and LF instead of CRLF. Comment lines beginning with `#` are not supported;
/// `#` within an IRI remains a fragment delimiter.
///
/// IRIs are validated but never normalized, so their original spelling survives
/// a round trip. Empty lines, whitespace, and control characters are rejected.
/// Validation checks absolute IRI syntax (RFC 3987), not scheme-specific rules.
/// With the `tokio` feature, `UriReader` borrows the first IRI from input while
/// ignoring trailing records; parsing this owned type requires exactly one IRI.
///
/// [RFC 2483, Section 5]: https://www.rfc-editor.org/rfc/rfc2483#section-5
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UriFile(String);

impl UriFile {
    /// Constructs file contents from an IRI without a trailing newline.
    pub fn new(uri: impl Into<String>) -> Result<Self, InvalidUriFile> {
        let uri = uri.into();
        validate_uri(&uri)?;
        Ok(Self(uri))
    }

    /// Returns the original IRI, without the file's trailing newline.
    pub fn uri(&self) -> &str {
        &self.0
    }
}

impl FromStr for UriFile {
    type Err = InvalidUriFile;

    fn from_str(contents: &str) -> Result<Self, Self::Err> {
        Self::new(contents.strip_suffix('\n').ok_or(InvalidUriFile)?)
    }
}

impl fmt::Display for UriFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.0)
    }
}

/// Invalid IRI input or malformed `.uri`/`.uris` file contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidUriFile;

impl fmt::Display for InvalidUriFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected absolute IRIs, one per line, with a final LF")
    }
}

impl core::error::Error for InvalidUriFile {}

pub(crate) fn validate_uri(uri: &str) -> Result<(), InvalidUriFile> {
    if uri.chars().any(|c| c.is_control() || c.is_whitespace())
        || iri_string::types::IriStr::new(uri).is_err()
    {
        return Err(InvalidUriFile);
    }
    Ok(())
}
