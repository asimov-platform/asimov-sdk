// This is free and unencumbered software released into the public domain.

//! Social graph primitives for ASIMOV.
//!
//! Use [`SocialHandle`] to associate an account handle with its platform,
//! [`SocialProperty`] to select account information, and [`FollowRelationship`]
//! to describe the direction of a follow relationship between accounts.
//! [`SocialLink`] recognizes URLs for profiles, relationships, and other social
//! resources, with fallible conversions to and from supported account handles.
//!
//! Platform modules re-export their platform-specific handle types. Each module
//! and its corresponding `SocialHandle` variant require the matching platform
//! feature; all platform features are enabled by default.

#![no_std]
#![forbid(unsafe_code)]
#![allow(unused_imports)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod collection_stub;
pub use collection_stub::*;

mod follow_relationship;
pub use follow_relationship::*;

mod social_handle;
pub use social_handle::*;

mod social_link;
pub use social_link::*;

mod social_platform;
pub use social_platform::*;

mod social_property;
pub use social_property::*;

#[cfg(feature = "facebook")]
/// Facebook-specific types, including account handles.
pub mod facebook;

#[cfg(feature = "github")]
/// GitHub-specific types, including account handles.
pub mod github;

#[cfg(feature = "gravatar")]
/// Gravatar-specific types, including account handles.
pub mod gravatar;

#[cfg(feature = "instagram")]
/// Instagram-specific types, including account handles.
pub mod instagram;

#[cfg(feature = "introco")]
/// Intro.co-specific types, including account handles.
pub mod introco;

#[cfg(feature = "linkedin")]
/// LinkedIn-specific types, including account handles.
pub mod linkedin;

#[cfg(feature = "localai")]
/// Local.ai-specific types, including account handles.
pub mod localai;

#[cfg(feature = "luma")]
/// Luma-specific types, including account handles.
pub mod luma;

#[cfg(feature = "telegram")]
/// Telegram-specific types, including account handles.
pub mod telegram;

#[cfg(feature = "whatsapp")]
/// WhatsApp-specific types, including account handles.
pub mod whatsapp;

#[cfg(feature = "x")]
/// X-specific account handles, profiles, and list identifiers.
pub mod x;
