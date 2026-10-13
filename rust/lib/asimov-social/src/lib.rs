// This is free and unencumbered software released into the public domain.

//! Social graph primitives for ASIMOV.
//!
//! Use [`SocialHandle`] to associate an account handle with its platform,
//! [`SocialProperty`] to select account information, and [`FollowRelationship`]
//! to describe the direction of a follow relationship between accounts.
//! [`SocialLink`] recognizes URLs for profiles, relationships, and other social
//! resources, with fallible conversions to and from supported account handles.
//! [`SocialPlatform::handle`] parses an identifier on a selected platform.
//! URL parsers accept HTTP(S), an optional `www.` hostname prefix, and known host
//! aliases; formatted URLs use HTTPS without `www.`.
//!
//! Platform modules re-export their platform-specific handle types. Each module
//! and its corresponding `SocialHandle` variant require the matching platform
//! feature; all platform features are enabled by default.
//!
//! [`FetchRequest`] and [`ListRequest`] share asimov-remote's transport-independent
//! request types and options. The `serde` feature enables their JSON encoding
//! without an HTTP client; `client` enables `SocialClient` for HTTP execution.
//! Caching, filtering, and timing are configured on the requests and encoded
//! alongside pattern options in the JSON `options` object.
//!
//! ```
//! # #[cfg(feature = "serde")] {
//! use asimov_social::{FetchRequest, ListRequest};
//!
//! let fetch = FetchRequest::new("https://example.com/profile", Default::default());
//! let json = serde_json::to_string(&fetch).unwrap();
//! assert_eq!(serde_json::from_str::<FetchRequest>(&json).unwrap(), fetch);
//!
//! let list: ListRequest = serde_json::from_str(
//!     r#"{"url":"https://example.com/collection","options":{"limit":0}}"#,
//! ).unwrap();
//! assert_eq!(list.options.limit, Some(0));
//! # }
//! ```

#![no_std]
#![forbid(unsafe_code)]
#![allow(unused_imports)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub use asimov_remote::{CachingOptions, FilteringOptions, TimingOptions};

mod collection_stub;
pub use collection_stub::*;

mod fetch_request;
pub use fetch_request::*;

mod follow_relationship;
pub use follow_relationship::*;

mod platform_handles;
pub use platform_handles::*;

#[cfg(test)]
mod platform_tests;

mod list_request;
pub use list_request::*;

#[cfg(feature = "client")]
mod social_client;
#[cfg(feature = "client")]
pub use social_client::*;

mod social_handle;
pub use social_handle::*;

mod social_link;
pub use social_link::*;

mod social_platform;
pub use social_platform::*;

mod social_property;
pub use social_property::*;

mod social_url;

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
