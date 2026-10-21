// This is free and unencumbered software released into the public domain.

use super::{ModuleManifestIterator, ModuleNameIterator, StateDirectory};
use alloc::format;
use camino::{Utf8Path, Utf8PathBuf};
use derive_more::Display;
use std::{
    io::{Error, ErrorKind, Result},
    path::Path,
};

/// A module directory stored on a file system (e.g., `$HOME/.asimov/modules/`).
#[derive(Debug, Display)]
#[display("ModulesDirectory({:?})", path)]
pub struct ModulesDirectory {
    pub(crate) path: Utf8PathBuf,
}

impl AsRef<Utf8PathBuf> for ModulesDirectory {
    fn as_ref(&self) -> &Utf8PathBuf {
        &self.path
    }
}

impl ModulesDirectory {
    /// Opens the default module directory in the user's home directory.
    ///
    /// On Unix platforms, including macOS and Linux, this is `$HOME/.asimov/modules/`.
    pub fn home() -> Result<Self> {
        StateDirectory::home().map(|base_dir| base_dir.modules())?
    }

    /// Opens a module directory from a file system path.
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
        Ok(ModulesDirectory { path })
    }

    pub async fn iter_enabled(&self) -> Result<ModuleNameIterator> {
        ModuleNameIterator::new(self.join("enabled")).await
    }

    pub async fn iter_installed(&self) -> Result<ModuleNameIterator> {
        ModuleNameIterator::new(self.join("installed")).await
    }

    pub async fn iter_manifests(&self) -> Result<ModuleManifestIterator> {
        ModuleManifestIterator::new(self.join("installed")).await
    }

    pub fn join(&self, path: impl AsRef<Utf8Path>) -> Utf8PathBuf {
        self.path.join(path.as_ref())
    }

    pub fn as_str(&self) -> &str {
        self.path.as_str()
    }
}

impl AsRef<str> for ModulesDirectory {
    fn as_ref(&self) -> &str {
        self.path.as_str()
    }
}

impl AsRef<Path> for ModulesDirectory {
    fn as_ref(&self) -> &Path {
        self.path.as_std_path()
    }
}

#[cfg(feature = "camino")]
impl AsRef<Utf8Path> for ModulesDirectory {
    fn as_ref(&self) -> &Utf8Path {
        self.path.as_path()
    }
}

impl crate::ModulesDirectory for ModulesDirectory {
    fn is_installed(&self, _module_name: impl AsRef<str>) -> bool {
        false // TODO
    }

    fn is_enabled(&self, _module_name: impl AsRef<str>) -> bool {
        false // TODO
    }
}
