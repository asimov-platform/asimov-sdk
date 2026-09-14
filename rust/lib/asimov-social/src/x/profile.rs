// This is free and unencumbered software released into the public domain.

use super::XHandle;

/// An X account profile identified by its handle.
///
/// With the `serde` feature, this type supports serialization and deserialization.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct XProfile {
    /// The account's handle on X, represented by the platform-specific handle type.
    pub handle: XHandle,
}
