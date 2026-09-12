// This is free and unencumbered software released into the public domain.

//! Social graph primitives for ASIMOV.
//!
//! This crate provides [`FollowRelationship`] for describing the direction of
//! a follow relationship between the current account and another account.

#![no_std]
#![forbid(unsafe_code)]
#![allow(unused_imports)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod follow_relationship;
pub use follow_relationship::*;

#[cfg(feature = "facebook")]
pub mod facebook;

#[cfg(feature = "github")]
pub mod github;

#[cfg(feature = "gravatar")]
pub mod gravatar;

#[cfg(feature = "instagram")]
pub mod instagram;

#[cfg(feature = "introco")]
pub mod introco;

#[cfg(feature = "linkedin")]
pub mod linkedin;

#[cfg(feature = "localai")]
pub mod localai;

#[cfg(feature = "luma")]
pub mod luma;

#[cfg(feature = "telegram")]
pub mod telegram;

#[cfg(feature = "whatsapp")]
pub mod whatsapp;

#[cfg(feature = "x")]
pub mod x;
