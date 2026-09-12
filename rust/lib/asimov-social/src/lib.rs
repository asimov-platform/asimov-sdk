// This is free and unencumbered software released into the public domain.

//! Social graph primitives for ASIMOV.
//!
//! This crate provides [`FollowRelationship`] for describing the direction of
//! a follow relationship between the current account and another account.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod follow_relationship;
pub use follow_relationship::*;
