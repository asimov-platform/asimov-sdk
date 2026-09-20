// This is free and unencumbered software released into the public domain.

use alloc::{string::String, vec::Vec};

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct FetchRequest {
    pub urls: Vec<String>,

    #[cfg_attr(
        feature = "serde",
        serde(skip_serializing_if = "FetchRequestOptions::is_empty")
    )]
    pub options: FetchRequestOptions,
}

impl FetchRequest {
    /// Parses a request from JSON.
    #[cfg(feature = "serde")]
    pub fn from_json(input: impl AsRef<str>) -> serde_json::Result<Self> {
        serde_json::from_str(input.as_ref())
    }

    /// Serializes this request to JSON.
    #[cfg(feature = "serde")]
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct FetchRequestOptions {}

impl FetchRequestOptions {
    pub fn is_empty(&self) -> bool {
        true
    }

    /// Parses request options from JSON.
    #[cfg(feature = "serde")]
    pub fn from_json(input: impl AsRef<str>) -> serde_json::Result<Self> {
        serde_json::from_str(input.as_ref())
    }

    /// Serializes these request options to JSON.
    #[cfg(feature = "serde")]
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}
