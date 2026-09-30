// This is free and unencumbered software released into the public domain.

//! Standard output formats with application-defined extensions.

use alloc::string::String;
use core::{fmt, str::FromStr};

/// An output format for a listing.
///
/// The standard command-line names are `jsonl` and `url`. Other names are
/// parsed through `T`, which defaults to [`String`] to accept arbitrary names.
/// Use an enum implementing [`FromStr`] to restrict custom formats.
/// [`Display`](fmt::Display) delegates custom format names to `T`.
///
/// The default is [`Jsonl`](Self::Jsonl). An unset
/// [`crate::ListerOptions::output`] still omits the option entirely.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum OutputFormat<T = String> {
    /// Line-delimited JSON (`jsonl`).
    #[default]
    Jsonl,
    /// Resource URLs (`url`).
    Url,
    /// An application-defined format.
    Other(T),
}

impl<T: fmt::Display> fmt::Display for OutputFormat<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Jsonl => f.write_str("jsonl"),
            Self::Url => f.write_str("url"),
            Self::Other(value) => value.fmt(f),
        }
    }
}

impl<T: FromStr> FromStr for OutputFormat<T> {
    type Err = T::Err;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "jsonl" => Ok(Self::Jsonl),
            "url" => Ok(Self::Url),
            _ => value.parse().map(Self::Other),
        }
    }
}

impl<T: AsRef<str>> OutputFormat<T> {
    /// Returns the command-line format name without allocating.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Jsonl => "jsonl",
            Self::Url => "url",
            Self::Other(value) => value.as_ref(),
        }
    }
}

impl<T: AsRef<str>> AsRef<str> for OutputFormat<T> {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<String> for OutputFormat<String> {
    fn from(value: String) -> Self {
        match value.as_str() {
            "jsonl" => Self::Jsonl,
            "url" => Self::Url,
            _ => Self::Other(value),
        }
    }
}

impl From<&str> for OutputFormat<String> {
    fn from(value: &str) -> Self {
        match value {
            "jsonl" => Self::Jsonl,
            "url" => Self::Url,
            _ => Self::Other(value.into()),
        }
    }
}
