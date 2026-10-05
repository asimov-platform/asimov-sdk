// This is free and unencumbered software released into the public domain.

//! Universe registry for ASIMOV.
//!
//! This crate is a scaffold and currently exposes no universe-registry API.
//! Universe registration, discovery, and lookup have no public contract here yet.
//!
//! The crate supports `no_std`; enabling `std` forwards standard-library support
//! to its dependencies but does not provide a registry backend.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;
