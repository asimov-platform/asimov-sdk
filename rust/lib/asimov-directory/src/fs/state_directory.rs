// This is free and unencumbered software released into the public domain.

use super::{ConfigDirectory, ModuleDirectory, ProgramDirectory, RemoteDirectory};
use alloc::format;
use asimov_core::crates::cap_std;
use camino::{Utf8Path, Utf8PathBuf};
use derive_more::Display;
use std::{
    io::{Error, ErrorKind, Result},
    path::Path,
};

/// A state directory stored on a file system (e.g., `$HOME/.asimov/`).
#[derive(Debug, Display)]
#[display("StateDirectory({:?})", path)]
pub struct StateDirectory {
    path: Utf8PathBuf,
}

impl AsRef<Utf8PathBuf> for StateDirectory {
    fn as_ref(&self) -> &Utf8PathBuf {
        &self.path
    }
}

impl StateDirectory {
    /// Opens `ASIMOV_ROOT`, falling back to the default home state directory.
    pub fn from_env() -> Result<Self> {
        match std::env::var_os("ASIMOV_ROOT") {
            Some(path) if path.is_empty() => Err(Error::new(
                ErrorKind::InvalidInput,
                "ASIMOV_ROOT must not be empty",
            )),
            Some(path) => Self::open(path),
            None => Self::home(),
        }
    }

    /// Opens the default state directory in the user's home directory.
    ///
    /// On Unix platforms, including macOS and Linux, this is `$HOME/.asimov/`.
    pub fn home() -> Result<Self> {
        let home_dir = std::env::home_dir()
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "home directory not found"))?;
        Self::open(home_dir.join(".asimov"))
    }

    /// Opens a state directory from a file system path.
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
        Ok(StateDirectory { path })
    }

    /// Opens the configuration directory under this state directory.
    pub fn configs(&self) -> Result<ConfigDirectory> {
        ConfigDirectory::open(self.join("configs"))
    }

    /// Opens the module directory under this state directory.
    pub fn modules(&self) -> Result<ModuleDirectory> {
        ModuleDirectory::open(self.join("modules"))
    }

    /// Opens the program directory under this state directory.
    pub fn programs(&self) -> Result<ProgramDirectory> {
        ProgramDirectory::open(self.join("libexec"))
    }

    /// Opens remote storage rooted at this state directory.
    ///
    /// The `remotes/` subdirectory is created only when adding a remote.
    pub fn remotes(&self) -> Result<RemoteDirectory> {
        let root = cap_std::fs::Dir::open_ambient_dir(
            self.path.as_std_path(),
            cap_std::ambient_authority(),
        )?;
        Ok(RemoteDirectory::new(root))
    }

    pub fn join(&self, path: impl AsRef<Utf8Path>) -> Utf8PathBuf {
        self.path.join(path.as_ref())
    }

    pub fn as_str(&self) -> &str {
        self.path.as_str()
    }
}

impl AsRef<str> for StateDirectory {
    fn as_ref(&self) -> &str {
        self.path.as_str()
    }
}

impl AsRef<Path> for StateDirectory {
    fn as_ref(&self) -> &Path {
        self.path.as_std_path()
    }
}

#[cfg(feature = "camino")]
impl AsRef<Utf8Path> for StateDirectory {
    fn as_ref(&self) -> &Utf8Path {
        self.path.as_path()
    }
}

impl crate::StateDirectory for StateDirectory {
    fn has_configs(&self) -> bool {
        self.path.join("configs").exists()
    }

    fn has_modules(&self) -> bool {
        self.path.join("modules").exists()
    }

    fn has_programs(&self) -> bool {
        self.path.join("libexec").exists()
    }
}
