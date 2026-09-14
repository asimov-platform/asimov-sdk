// This is free and unencumbered software released into the public domain.

//! Re-exports the types from `known_types_gravatar`, including [`GravatarHandle`].
//!
//! Use that handle type for Gravatar-specific data, or wrap it in
//! [`SocialHandle::Gravatar`](crate::SocialHandle::Gravatar) when working across
//! platforms. [`SocialHandle::gravatar`](crate::SocialHandle::gravatar) parses
//! a handle directly into the platform-tagged representation.
//!
//! Available with the `gravatar` feature, which is enabled by default.
//!
//! [`GravatarHandle`]: crate::gravatar::GravatarHandle

pub use known_types_gravatar::GravatarHandle;
