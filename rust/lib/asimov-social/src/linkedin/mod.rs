// This is free and unencumbered software released into the public domain.

//! Re-exports the types from `known_types_linkedin`, including [`LinkedinHandle`].
//!
//! Use that handle type for LinkedIn-specific data, or wrap it in
//! [`SocialHandle::Linkedin`](crate::SocialHandle::Linkedin) when working across
//! platforms. [`SocialHandle::linkedin`](crate::SocialHandle::linkedin) parses
//! a handle directly into the platform-tagged representation.
//!
//! Available with the `linkedin` feature, which is enabled by default.
//!
//! [`LinkedinHandle`]: crate::linkedin::LinkedinHandle

pub use known_types_linkedin::LinkedinHandle;
