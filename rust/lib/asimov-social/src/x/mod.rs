// This is free and unencumbered software released into the public domain.

//! Re-exports [`XHandle`] from `known_types_x` and provides [`XProfile`] for
//! handle-based account profiles and [`XList`] for numeric list identifiers.
//!
//! Use [`XHandle`] for X-specific data, or wrap it in
//! [`SocialHandle::X`](crate::SocialHandle::X) when working across platforms.
//! [`SocialHandle::x`](crate::SocialHandle::x) parses a handle directly into
//! the platform-tagged representation.
//!
//! Available with the `x` feature, which is enabled by default.
//!
//! [`XHandle`]: crate::x::XHandle
//! [`XProfile`]: crate::x::XProfile
//! [`XList`]: crate::x::XList

pub use known_types_x::XHandle;

mod list;
pub use list::*;

mod profile;
pub use profile::*;

mod post;
pub use post::*;
