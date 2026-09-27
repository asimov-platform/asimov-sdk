// This is free and unencumbered software released into the public domain.

//! Shared argument groups for host commands.
//!
//! Argument doc comments document the Rust API. Set explicit Clap `help` and
//! `long_help` text for CLI users: Clap otherwise derives extended help from
//! the full doc comment, including implementation details and rustdoc links.
//! Suppress inferred `about` and `long_about` on argument groups as well, so
//! their Rust docs do not become the containing command's description.

mod caching;
pub use caching::*;

mod filtering;
pub use filtering::*;

mod timing;
pub use timing::*;
