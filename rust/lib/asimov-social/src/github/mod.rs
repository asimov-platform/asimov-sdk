// This is free and unencumbered software released into the public domain.

//! Re-exports the types from `known_types_github`, including [`GithubHandle`].
//!
//! Use that handle type for GitHub-specific data, or wrap it in
//! [`SocialHandle::Github`](crate::SocialHandle::Github) when working across
//! platforms. [`SocialHandle::github`](crate::SocialHandle::github) parses
//! a handle directly into the platform-tagged representation.
//!
//! Available with the `github` feature, which is enabled by default.
//!
//! [`GithubHandle`]: crate::github::GithubHandle

pub use known_types_github::GithubHandle;
