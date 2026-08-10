// This is free and unencumbered software released into the public domain.

use std::{io, path::PathBuf, string::String};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CreateFileTreeError {
    #[error("failed to create directory for installed modules `{0}`: {1}")]
    InstallDir(PathBuf, #[source] io::Error),
    #[error("failed to create directory for enabled modules `{0}`: {1}")]
    EnableDir(PathBuf, #[source] io::Error),
    #[error("failed to create directory for module binaries `{0}`: {1}")]
    ExecDir(PathBuf, #[source] io::Error),
}

#[derive(Debug, Error)]
pub enum AddModuleError {
    #[error("module is already installed")]
    AlreadyInstalled,
    #[error("failed to install module from `{0}` to `{1}`: {2}")]
    Install(PathBuf, PathBuf, #[source] io::Error),
    #[error("failed to read directory of module binaries `{0}`: {1}")]
    ReadBinDir(PathBuf, #[source] io::Error),
    #[error("failed to add module binary `{0}`: {1}")]
    AddBinary(String, #[source] io::Error),
}

#[derive(Debug, Error)]
pub enum ReadReadmeError {
    #[error("module is not installed")]
    NotInstalled,
    #[error("error while checking whether module is installed: {0}")]
    IsModuleInstalled(#[from] IsModuleInstalledError),
    #[error("failed to read module README at `{0}`: {1}")]
    Read(PathBuf, #[source] io::Error),
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("module is not installed")]
    NotInstalled,
    #[error("error while searching for manifest file: {0}")]
    FindManifest(#[from] FindManifestError),
    #[error("unable to read module manifest: {0}")]
    Read(#[from] ReadManifestError),
}

#[derive(Debug, Error)]
#[error(transparent)]
pub struct ModuleVersionError(#[from] ManifestError);

#[derive(Debug, Error)]
pub enum RemoveModuleError {
    #[error("error while searching for manifest file: {0}")]
    FindManifest(#[from] FindManifestError),
    #[error("module is not installed")]
    NotInstalled,
    #[error("failed to remove directory of installed module `{0}`: {1}")]
    RemoveModuleDir(PathBuf, #[source] io::Error),
}

#[derive(Debug, Error)]
pub enum RemoveBinaryError {
    #[error("invalid program name `{0}`")]
    InvalidName(String),
    #[error("failed to remove binary: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Error)]
pub enum InstalledModulesError {
    #[error("failed to read directory for installed modules `{0}`: {1}")]
    DirIo(PathBuf, #[source] io::Error),
    #[error("failed to read manifest at `{0}`: {1}")]
    ReadManifestError(PathBuf, #[source] ReadManifestError),
}

#[derive(Debug, Error)]
pub enum EnabledModulesError {
    #[error("failed to read directory for enabled modules `{0}`: {1}")]
    DirIo(PathBuf, #[source] io::Error),
    #[error("failed to read manifest at `{0}`: {1}")]
    ReadManifestError(PathBuf, #[source] ReadManifestError),
}

#[derive(Debug, Error)]
#[error("error while searching for manifest file: {0}")]
pub struct IsModuleInstalledError(#[from] FindManifestError);

#[derive(Debug, Error)]
#[error("unable to read symlink for enabled module: {0}")]
pub struct IsModuleEnabledError(#[from] io::Error);

#[derive(Debug, Error)]
pub enum EnableError {
    #[error("module is not installed")]
    NotInstalled,
    #[error("failed to enable module: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Error)]
#[error("failed to disable module: {0}")]
pub struct DisableError(#[from] pub io::Error);

mod common {
    use super::*;

    #[derive(Debug, Error)]
    pub enum ReadManifestError {
        #[error("failed to access module manifest file: {0}")]
        InstalledManifestIo(#[from] io::Error),
        #[error("failed to deserialize module manifest: {0}")]
        ManifestDeserialize(#[from] DeserializeError),
    }

    impl From<serde_json::Error> for ReadManifestError {
        fn from(value: serde_json::Error) -> Self {
            ReadManifestError::ManifestDeserialize(DeserializeError::Json(value))
        }
    }

    impl From<serde_yaml_ng::Error> for ReadManifestError {
        fn from(value: serde_yaml_ng::Error) -> Self {
            ReadManifestError::ManifestDeserialize(DeserializeError::Yaml(value))
        }
    }

    #[derive(Debug, Error)]
    pub enum DeserializeError {
        #[error("JSON deserialization failed: {0}")]
        Json(#[from] serde_json::Error),
        #[error("YAML deserialization failed: {0}")]
        Yaml(#[from] serde_yaml_ng::Error),
    }

    #[derive(Debug, Error)]
    #[error("unable to check for manifest file at `{0}`: {1}")]
    pub struct FindManifestError(pub PathBuf, #[source] pub io::Error);

    #[derive(Debug, Error)]
    pub enum SerializeError {
        #[error("JSON serialization failed: {0}")]
        Json(#[from] serde_json::Error),
        #[error("YAML serialization failed: {0}")]
        Yaml(#[from] serde_yaml_ng::Error),
    }
}
pub use common::*;
