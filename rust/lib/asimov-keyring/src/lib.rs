// This is free and unencumbered software released into the public domain.

//! Keyring support for ASIMOV.
//!
//! Public keys are read from `$HOME/.asimov/keyring/<user>` without accessing
//! the secret-key backend. The backend is opened only to initialize a missing
//! identity or perform an explicit secret-key operation.
//!
//! On Linux, the default backend is the Secret Service, such as GNOME Keyring
//! or KDE Wallet, which stores secret keys as Base58 text. It uses private
//! files when no Secret Service is available on the session bus, as on
//! headless machines and in containers. No environment configuration is
//! needed. Once the file store exists, it is reused even if a Secret Service
//! later becomes available, preserving identity.
//! Other platforms default to their native backend.
//!
//! File secrets are stored in `$HOME/.asimov/.keyring/<user>/secret` as
//! unencrypted key bytes, protected by directory mode 0700 and file mode 0600.
//! Mount the user's `.asimov` directory on persistent storage to retain the
//! identity across container recreation. Like the native keyring's public-key
//! cache, this path uses the home directory, not `ASIMOV_ROOT`.
//!
//! `ASIMOV_KEYRING_BACKEND=native` forces the platform backend without fallback;
//! `ASIMOV_KEYRING_BACKEND=file` forces file storage on Unix. Native and file
//! secrets are separate; initializing an empty backend creates a new identity.
//! Automatic fallback only handles an unavailable Secret Service on opening;
//! subsequent secret-read, corruption, and write errors are returned as errors.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
mod keyring;
#[cfg(feature = "std")]
pub use keyring::*;

#[cfg(feature = "std")]
mod store;

#[cfg(all(feature = "std", unix))]
mod file_store;

mod keyring_error;
pub use keyring_error::*;
