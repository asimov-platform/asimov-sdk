// This is free and unencumbered software released into the public domain.

//! Re-exports the types from `known_types_luma`, including [`LumaHandle`].
//!
//! Use that handle type for Luma-specific data, or wrap it in
//! [`SocialHandle::Luma`](crate::SocialHandle::Luma) when working across
//! platforms. [`SocialHandle::luma`](crate::SocialHandle::luma) parses
//! a handle directly into the platform-tagged representation.
//!
//! Available with the `luma` feature, which is enabled by default.
//!
//! [`LumaHandle`]: crate::luma::LumaHandle

pub use known_types_luma::LumaHandle;
