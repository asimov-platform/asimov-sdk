// This is free and unencumbered software released into the public domain.

use alloc::string::String;

/// A stub for a collection, containing only the count of items.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(tag = "@type", content = "Collection", default)
)]
pub struct CollectionStub {
    /// The optional ID of the collection.
    #[cfg_attr(
        feature = "serde",
        serde(rename = "@id", skip_serializing_if = "Option::is_none")
    )]
    pub id: Option<String>,

    /// The count of items in the collection, if known.
    pub count: Option<u64>,
}

impl From<u64> for CollectionStub {
    fn from(value: u64) -> Self {
        Self {
            id: None,
            count: Some(value),
        }
    }
}

impl From<usize> for CollectionStub {
    fn from(value: usize) -> Self {
        Self {
            id: None,
            count: Some(value as u64),
        }
    }
}

impl From<CollectionStub> for u64 {
    fn from(value: CollectionStub) -> Self {
        value.count.unwrap_or_default()
    }
}

impl From<CollectionStub> for usize {
    fn from(value: CollectionStub) -> Self {
        value.count.unwrap_or_default() as usize
    }
}
