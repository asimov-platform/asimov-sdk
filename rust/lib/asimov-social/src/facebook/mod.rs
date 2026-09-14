// This is free and unencumbered software released into the public domain.

//! Re-exports the types from `known_types_facebook`, including [`FacebookHandle`].
//!
//! Use that handle type for Facebook-specific data, or wrap it in
//! [`SocialHandle::Facebook`](crate::SocialHandle::Facebook) when working across
//! platforms. [`SocialHandle::facebook`](crate::SocialHandle::facebook) parses
//! a handle directly into the platform-tagged representation.
//!
//! Available with the `facebook` feature, which is enabled by default.
//!
//! [`FacebookHandle`]: crate::facebook::FacebookHandle

pub use known_types_facebook::FacebookHandle;
