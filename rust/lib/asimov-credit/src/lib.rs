// This is free and unencumbered software released into the public domain.

//! Decimal-backed credit amounts for ASIMOV.
//!
//! [`Credits`] supports parsing, formatting, arithmetic assignment, and numeric
//! conversions. Display uses nine fractional digits; the `serde` feature adds
//! string-based serialization using that display format. Arbitrary decimal
//! inputs are not constrained to nanocredit range or precision, so these
//! conversions do not yet guarantee lossless round trips.
//!
//! The `eloquent`, `libsql`, and `turso` features currently only enable their
//! dependencies. They provide no database mappings or persistence API for
//! [`Credits`]. A first database integration needs an exact storage encoding,
//! checked decoding errors, and round-trip tests at range and precision limits.
//! The `clap` feature likewise only enables its dependency; it adds no custom
//! argument parser.
//!
//! The crate uses `no_std`; `std` enables standard-library support in core and
//! decimal dependencies. Optional integrations may require `std` independently.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod credits;
pub use credits::*;

mod error;
pub use error::*;
