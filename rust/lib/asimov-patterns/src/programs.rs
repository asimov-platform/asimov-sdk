// This is free and unencumbered software released into the public domain.

//! ASIMOV program patterns.
//!
//! See: https://asimov-specs.github.io/program-patterns/

mod adapter;
pub use adapter::*;

mod lister;
pub use lister::*;

mod emitter;
pub use emitter::*;

mod fetcher;
pub use fetcher::*;

mod indexer;
pub use indexer::*;

mod matcher;
pub use matcher::*;

mod prompter;
pub use prompter::*;

mod reader;
pub use reader::*;

mod reasoner;
pub use reasoner::*;

mod resolver;
pub use resolver::*;

mod runner;
pub use runner::*;

mod writer;
pub use writer::*;
