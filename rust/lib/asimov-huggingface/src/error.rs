// This is free and unencumbered software released into the public domain.

use hf_hub::api::sync::ApiError;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HuggingfaceError {
    #[error("failed to access Hugging Face API: {0}")]
    Api(#[from] ApiError),
    #[error("snapshot is empty")]
    EmptySnapshot,
    #[error("failed to download file `{0}`")]
    Download(PathBuf),
    /// Shared proxy discovery or target-origin validation failed.
    #[error(transparent)]
    Proxy(#[from] asimov_http::proxy::Error),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid Hugging Face input: {0}")]
    InvalidInput(&'static str),
    #[error("invalid Hugging Face response: {0}")]
    InvalidResponse(&'static str),
    #[error("Hugging Face download worker panicked")]
    Worker,
}

impl From<std::io::Error> for HuggingfaceError {
    fn from(error: std::io::Error) -> Self {
        Self::Api(ApiError::IoError(error))
    }
}

pub type Result<T> = core::result::Result<T, HuggingfaceError>;
