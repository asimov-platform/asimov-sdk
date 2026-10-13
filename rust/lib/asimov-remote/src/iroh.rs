// This is free and unencumbered software released into the public domain.

//! Reserved for remote execution over Iroh/QUIC.
//!
//! The `iroh` feature currently exposes only this placeholder module. It
//! provides no executor or wire protocol. A future adapter will define its own
//! request mapping, authentication, response framing, and failure semantics.
//!
//! [`crate::FetcherRequest`] and [`crate::ListerRequest`] describe operations
//! independently of this transport. Their JSON encodings do not establish an
//! Iroh protocol, and the HTTP adapter's protocol is specific to that adapter.
