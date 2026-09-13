// This is free and unencumbered software released into the public domain.

use super::XHandle;

/// X account profile.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct XProfile {
    /// The handle of the user on X.
    pub handle: XHandle,
}
