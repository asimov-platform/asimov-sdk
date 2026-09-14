// This is free and unencumbered software released into the public domain.

//! Re-exports the types from `known_types_instagram`, including [`InstagramHandle`].
//!
//! Use that handle type for Instagram-specific data, or wrap it in
//! [`SocialHandle::Instagram`](crate::SocialHandle::Instagram) when working across
//! platforms. [`SocialHandle::instagram`](crate::SocialHandle::instagram) parses
//! a handle directly into the platform-tagged representation.
//!
//! Available with the `instagram` feature, which is enabled by default.
//!
//! [`InstagramHandle`]: crate::instagram::InstagramHandle

pub use known_types_instagram::InstagramHandle;
