// This is free and unencumbered software released into the public domain.

//! Runtime support for ASIMOV.
//!
//! This crate is a scaffold and currently exposes no runtime API. Scheduling,
//! task execution, and runtime lifecycle have no public contract here yet.
//!
//! The crate supports `no_std`; enabling `std` forwards standard-library support
//! to its dependencies but does not provide an executor or start a runtime.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;
