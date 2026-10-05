// This is free and unencumbered software released into the public domain.

//! Dataset management for ASIMOV.
//!
//! This crate is a scaffold and currently exposes no dataset API. Dataset
//! representation, storage, and import/export have no public contract here yet.
//!
//! The crate supports `no_std`; enabling `std` forwards standard-library support
//! to its dependencies but does not provide dataset storage.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;
