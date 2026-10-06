// This is free and unencumbered software released into the public domain.

//! Keyring support for ASIMOV.
//!
//! On Linux, the default backend uses private files when access to the kernel
//! keyrings is unavailable, including under Docker's default seccomp policy.
//! No environment configuration is needed. Once the file store exists, it is
//! reused even if kernel keyrings later become available, preserving identity.
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
//! secrets are separate; an empty backend creates a new identity. Automatic
//! fallback only handles unavailable kernel-keyring access during opening;
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
