// This is free and unencumbered software released into the public domain.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "fs")]
pub mod fs;

pub use asimov_config::{ConfigProfile, ConfigsDirectory};

mod modules_directory;
pub use modules_directory::*;

mod module_iterators;
pub use module_iterators::*;

mod programs_directory;
pub use programs_directory::*;

mod state_directory;
pub use state_directory::*;

mod uri_file;
pub use uri_file::*;

#[cfg(feature = "tokio")]
mod uri_reader;
#[cfg(feature = "tokio")]
pub use uri_reader::*;

mod uris_file;
pub use uris_file::*;

#[cfg(feature = "tokio")]
mod uris_reader;
#[cfg(feature = "tokio")]
pub use uris_reader::*;
