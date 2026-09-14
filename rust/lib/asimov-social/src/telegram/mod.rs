// This is free and unencumbered software released into the public domain.

//! Re-exports the types from `known_types_telegram`, including [`TelegramHandle`].
//!
//! Use that handle type for Telegram-specific data, or wrap it in
//! [`SocialHandle::Telegram`](crate::SocialHandle::Telegram) when working across
//! platforms. [`SocialHandle::telegram`](crate::SocialHandle::telegram) parses
//! a handle directly into the platform-tagged representation.
//!
//! Available with the `telegram` feature, which is enabled by default.
//!
//! [`TelegramHandle`]: crate::telegram::TelegramHandle

pub use known_types_telegram::TelegramHandle;
