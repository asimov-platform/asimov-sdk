// This is free and unencumbered software released into the public domain.

//! Vault access for ASIMOV.
//!
//! This crate is a scaffold and currently exposes no vault API. Vault storage,
//! access control, and backend integration have no public contract here yet.
//!
//! The crate supports `no_std`; enabling `std` forwards standard-library support
//! to its dependencies but does not provide vault clients or storage.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;
