// This is free and unencumbered software released into the public domain.

//! Re-exports the types from `known_types_localai`, including [`LocalaiHandle`].
//!
//! Use that handle type for Local.ai-specific data, or wrap it in
//! [`SocialHandle::Localai`](crate::SocialHandle::Localai) when working across
//! platforms. [`SocialHandle::localai`](crate::SocialHandle::localai) parses
//! a handle directly into the platform-tagged representation.
//!
//! Available with the `localai` feature, which is enabled by default.
//!
//! [`LocalaiHandle`]: crate::localai::LocalaiHandle

pub use known_types_localai::LocalaiHandle;
