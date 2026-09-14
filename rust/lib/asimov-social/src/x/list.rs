// This is free and unencumbered software released into the public domain.

use super::XHandle;

/// An X list identified by its numeric ID.
///
/// With the `serde` feature, this type supports serialization and deserialization.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct XList {
    /// The platform-assigned list ID.
    ///
    /// The value is stored as supplied; this type does not validate its range
    /// or check whether the list exists on X.
    pub id: i64,
}
