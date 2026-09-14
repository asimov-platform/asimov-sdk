// This is free and unencumbered software released into the public domain.

use asimov_id::KeyError;
use thiserror::Error;

/// An error locating a user, accessing key storage, or decoding a key.
///
/// I/O, keyring-backend, and key-decoding errors retain their underlying error
/// as a source and can be converted into this type with [`From`].
#[derive(Debug, Error)]
pub enum KeyringError {
    /// The requested user could not be found.
    #[error("user not found")]
    UserNotFound,

    /// A filesystem or other I/O operation failed.
    ///
    /// Available with the `std` feature.
    #[cfg(feature = "std")]
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// The keyring backend failed to initialize or access a secret-key entry.
    #[error("keyring error: {0}")]
    KeyringError(#[from] keyring_core::Error),

    /// A key could not be decoded, for example from a stored public-key file.
    #[error("key error: {0}")]
    KeyError(#[from] KeyError),
}
