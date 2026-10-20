// This is free and unencumbered software released into the public domain.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "fs")]
pub mod fs;

pub use asimov_config::{ConfigDirectory, ConfigProfile};

mod module_directory;
pub use module_directory::*;

mod module_iterators;
pub use module_iterators::*;

mod program_directory;
pub use program_directory::*;

mod state_directory;
pub use state_directory::*;

mod url_file;
pub use url_file::*;

mod urls_file;
pub use urls_file::*;

#[cfg(feature = "tokio")]
mod urls_reader;
#[cfg(feature = "tokio")]
pub use urls_reader::*;
