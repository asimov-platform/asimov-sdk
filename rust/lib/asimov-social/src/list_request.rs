// This is free and unencumbered software released into the public domain.

use alloc::{string::String, vec::Vec};

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct ListRequest {
    pub url: String,

    #[cfg_attr(
        feature = "serde",
        serde(skip_serializing_if = "ListRequestOptions::is_empty")
    )]
    pub options: ListRequestOptions,
}

impl ListRequest {
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
pub struct ListRequestOptions {
    pub offset: Option<u64>,
    pub limit: Option<u64>,
}

impl ListRequestOptions {
    pub fn is_empty(&self) -> bool {
        self.offset.is_none() && self.limit.is_none()
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
