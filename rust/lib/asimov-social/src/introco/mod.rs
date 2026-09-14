// This is free and unencumbered software released into the public domain.

//! Re-exports the types from `known_types_introco`, including [`IntrocoHandle`].
//!
//! Use that handle type for Intro.co-specific data, or wrap it in
//! [`SocialHandle::Introco`](crate::SocialHandle::Introco) when working across
//! platforms. [`SocialHandle::introco`](crate::SocialHandle::introco) parses
//! a handle directly into the platform-tagged representation.
//!
//! Available with the `introco` feature, which is enabled by default.
//!
//! [`IntrocoHandle`]: crate::introco::IntrocoHandle

pub use known_types_introco::IntrocoHandle;
