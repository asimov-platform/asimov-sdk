// This is free and unencumbered software released into the public domain.

//! Re-exports the types from `known_types_whatsapp`, including [`WhatsappHandle`].
//!
//! Use that handle type for WhatsApp-specific data, or wrap it in
//! [`SocialHandle::Whatsapp`](crate::SocialHandle::Whatsapp) when working across
//! platforms. [`SocialHandle::whatsapp`](crate::SocialHandle::whatsapp) parses
//! a handle directly into the platform-tagged representation.
//!
//! Available with the `whatsapp` feature, which is enabled by default.
//!
//! [`WhatsappHandle`]: crate::whatsapp::WhatsappHandle

pub use known_types_whatsapp::WhatsappHandle;
