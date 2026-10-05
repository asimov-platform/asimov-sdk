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

    /// A user name is not a portable, single filename component.
    #[error("invalid keyring user: {0}")]
    InvalidUser(#[from] asimov_core::InvalidFilenameComponent),

    /// A panic poisoned an in-process keyring coordination lock.
    #[cfg(feature = "std")]
    #[error("keyring coordination lock poisoned")]
    LockPoisoned,

    /// A stored secret has an invalid length; its contents are never included.
    #[error("corrupt secret key: expected 32 bytes, got {length}")]
    CorruptSecret {
        /// Number of bytes in the stored secret.
        length: usize,
    },

    /// A filesystem or other I/O operation failed.
    ///
    /// Available with the `std` feature.
    #[cfg(feature = "std")]
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Publishing a rekeyed public key and restoring the previous secret failed.
    ///
    /// The backend may retain the new secret. After resolving the storage
    /// failures, `Keyring::ensure_secret_key` repairs the cache from the stored
    /// secret without rotating it again.
    #[cfg(feature = "std")]
    #[error(
        "public-key publication failed: {cache_error}; secret rollback failed: {rollback_error}"
    )]
    RekeyRollbackFailed {
        /// The failure publishing the public-key cache.
        #[source]
        cache_error: std::io::Error,
        /// The failure restoring or removing the secret-key entry.
        rollback_error: keyring_core::Error,
    },

    /// The keyring backend failed to initialize or access a secret-key entry.
    #[error("keyring error: {0}")]
    KeyringError(#[from] keyring_core::Error),

    /// A key could not be decoded, for example from a stored public-key file.
    #[error("key error: {0}")]
    KeyError(#[from] KeyError),
}
