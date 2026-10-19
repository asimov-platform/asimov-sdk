// This is free and unencumbered software released into the public domain.

//! Hugging Face helpers for model downloads with unified progress bar.
//! Download and cache APIs require `std` (enabled by default).
//!
//! ```no_run
//! # #[cfg(feature = "std")] {
//! use asimov_huggingface::{ensure_file, ensure_snapshot};
//!
//! let file = ensure_file("facebook/dinov2-base", "pytorch_model.bin").unwrap();
//! let dir = ensure_snapshot("julien-c/dummy-unknown", None).unwrap();
//! # }
//! ```

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
mod cache;
#[cfg(feature = "std")]
mod download;
#[cfg(feature = "std")]
mod ensure;
#[cfg(feature = "std")]
mod http;
#[cfg(feature = "std")]
pub use ensure::*;

#[cfg(feature = "std")]
mod progress;
#[cfg(feature = "std")]
pub use progress::*;

#[cfg(feature = "std")]
mod error;
#[cfg(feature = "std")]
pub use error::{HuggingfaceError, Result};
