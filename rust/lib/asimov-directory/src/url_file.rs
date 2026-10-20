// This is free and unencumbered software released into the public domain.

use alloc::string::String;
use core::{fmt, str::FromStr};

/// The contents of a `.url` file: one URL followed by a newline.
///
/// URLs are validated but never normalized, so their original spelling survives
/// a round trip. Parsing requires an LF terminator; empty lines, whitespace,
/// and control characters in URLs are rejected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UrlFile(String);

impl UrlFile {
    /// Constructs file contents from a URL without a trailing newline.
    pub fn new(url: impl Into<String>) -> Result<Self, InvalidUrlFile> {
        let url = url.into();
        validate_url(&url)?;
        Ok(Self(url))
    }

    /// Returns the original URL, without the file's trailing newline.
    pub fn url(&self) -> &str {
        &self.0
    }
}

impl FromStr for UrlFile {
    type Err = InvalidUrlFile;

    fn from_str(contents: &str) -> Result<Self, Self::Err> {
        Self::new(contents.strip_suffix('\n').ok_or(InvalidUrlFile)?)
    }
}

impl fmt::Display for UrlFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.0)
    }
}

/// Invalid URL input or malformed `.url`/`.urls` file contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidUrlFile;

impl fmt::Display for InvalidUrlFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected absolute URLs, one per line, with a final newline")
    }
}

impl core::error::Error for InvalidUrlFile {}

pub(crate) fn validate_url(url: &str) -> Result<(), InvalidUrlFile> {
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) || url::Url::parse(url).is_err() {
        return Err(InvalidUrlFile);
    }
    Ok(())
}
