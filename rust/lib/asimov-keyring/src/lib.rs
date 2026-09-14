// This is free and unencumbered software released into the public domain.

//! Keyring support for ASIMOV.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
mod keyring;
#[cfg(feature = "std")]
pub use keyring::*;

mod keyring_error;
pub use keyring_error::*;
