// This is free and unencumbered software released into the public domain.

use super::StateDirectory;
use alloc::format;
use camino::{Utf8Path, Utf8PathBuf};
use derive_more::Display;
use std::{
    io::{Error, ErrorKind, Result},
    path::Path,
};

/// A program directory stored on a file system (e.g., `$HOME/.asimov/libexec/`).
#[derive(Debug, Display)]
#[display("ProgramsDirectory({:?})", path)]
pub struct ProgramsDirectory {
    path: Utf8PathBuf,
}

impl AsRef<Utf8PathBuf> for ProgramsDirectory {
    fn as_ref(&self) -> &Utf8PathBuf {
        &self.path
    }
}

impl ProgramsDirectory {
    /// Opens the default program directory in the user's home directory.
    ///
    /// On Unix platforms, including macOS and Linux, this is `$HOME/.asimov/libexec/`.
    pub fn home() -> Result<Self> {
        StateDirectory::home().map(|base_dir| base_dir.programs())?
    }

    /// Opens a program directory from a file system path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            std::fs::create_dir_all(path)?;
        }
        let path = Utf8PathBuf::from_path_buf(path.to_path_buf()).map_err(|e| {
            Error::new(
                ErrorKind::InvalidFilename,
                format!("failed to open non-UTF-8 path: {}", e.display()),
            )
        })?;
        Ok(ProgramsDirectory { path })
    }

    pub fn join(&self, path: impl AsRef<Utf8Path>) -> Utf8PathBuf {
        self.path.join(path.as_ref())
    }

    pub fn as_str(&self) -> &str {
        self.path.as_str()
    }
}

impl AsRef<str> for ProgramsDirectory {
    fn as_ref(&self) -> &str {
        self.path.as_str()
    }
}

impl AsRef<Path> for ProgramsDirectory {
    fn as_ref(&self) -> &Path {
        self.path.as_std_path()
    }
}

#[cfg(feature = "camino")]
impl AsRef<Utf8Path> for ProgramsDirectory {
    fn as_ref(&self) -> &Utf8Path {
        self.path.as_path()
    }
}

impl crate::ProgramsDirectory for ProgramsDirectory {}
