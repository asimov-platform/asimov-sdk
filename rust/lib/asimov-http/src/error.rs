// This is free and unencumbered software released into the public domain.

use alloc::string::String;

/// HTTP integrity construction, parsing, or verification failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A Structured Field is malformed or has the wrong semantic type.
    #[error("invalid integrity field: {0}")]
    InvalidField(&'static str),
    /// A field or derived component is unavailable in this message context.
    #[error("missing HTTP signature component: {0}")]
    MissingComponent(String),
    /// A component identifier or its parameters cannot be processed.
    #[error("invalid HTTP signature component: {0}")]
    InvalidComponent(String),
    /// The message context lacks a valid, absolute HTTP(S) target URI.
    #[error("invalid HTTP signature target URI")]
    InvalidTargetUri,
    /// Covered components must be unique, regardless of parameter ordering.
    #[error("duplicate covered component")]
    DuplicateComponent,
    /// Reusing a signature label would make selection ambiguous.
    #[error("duplicate signature label")]
    DuplicateLabel,
    /// SHA-256 was absent, malformed, or did not match the message content.
    #[error("missing or mismatched SHA-256 content digest")]
    ContentDigest,
    /// An algorithm other than Ed25519 was requested or asserted.
    #[error("only Ed25519 HTTP message signatures are supported")]
    Algorithm,
    /// Ed25519 verification failed.
    #[error("invalid HTTP message signature")]
    Signature,
    /// A required profile constraint was not satisfied.
    #[error("HTTP signature policy rejected the message: {0}")]
    Policy(&'static str),
    /// Integrity metadata exceeded the documented parsing limits.
    #[error("HTTP integrity metadata limit exceeded")]
    Limit,
    /// A frame appeared after trailers, or more than one trailer frame appeared.
    #[error("invalid HTTP body frame order")]
    FrameOrder,
    /// Streaming trailers cannot be used with these HTTP semantics.
    #[error("trailers are unavailable for this message")]
    TrailersUnavailable,
    /// OS entropy could not be obtained.
    #[error("failed to generate signature nonce: {0}")]
    Random(String),
    /// The wall clock cannot be represented as a nonnegative UNIX timestamp.
    #[error("invalid signature clock")]
    Clock,
}

impl From<sfv::Error> for Error {
    fn from(_: sfv::Error) -> Self {
        Self::InvalidField("structured field syntax")
    }
}
