// This is free and unencumbered software released into the public domain.

//! Repository access for ASIMOV.
//!
//! This crate is a scaffold and currently exposes no repository API. Repository
//! discovery, content access, and backend integration have no public contract
//! here yet.
//!
//! The crate supports `no_std`; enabling `std` forwards standard-library support
//! to its dependencies but does not provide repository clients.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;
