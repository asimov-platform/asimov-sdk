// This is free and unencumbered software released into the public domain.

//! Ontology types for ASIMOV.
//!
//! This crate is a scaffold and currently exposes no ontology types. Ontology
//! representation, vocabulary loading, and validation have no public contract
//! here yet.
//!
//! The crate supports `no_std`; enabling `std` forwards standard-library support
//! to its dependencies but does not add ontology functionality.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;
