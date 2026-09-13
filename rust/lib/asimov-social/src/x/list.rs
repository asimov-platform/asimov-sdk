// This is free and unencumbered software released into the public domain.

use super::XHandle;

/// X list.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct XList {
    /// The list's ID.
    pub id: i64,
}
