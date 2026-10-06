// This is free and unencumbered software released into the public domain.

use super::platform::PlatformInfo;
use asimov_registry::error as registry;
use std::{
    boxed::Box,
    io,
    path::PathBuf,
    string::{String, ToString as _},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InstallError {
    #[error(transparent)]
    WorkDir(#[from] WorkDirError),
    #[error(transparent)]
    Preinstall(#[from] PreinstallError),
    #[error(transparent)]
    Finish(#[from] FinishInstallError),
}

#[derive(Debug, Error)]
pub enum UpgradeError {
    #[error("upgrade publication or recovery failed: {0}")]
    Publication(#[from] io::Error),
    #[error("failed to check the latest version of module: {0}")]
    Fetch(#[from] FetchError),
    #[error("unable to read current version of module: {0}")]
    CheckVersion(#[from] registry::ModuleVersionError),
    #[error(transparent)]
    WorkDir(#[from] WorkDirError),
    #[error("unable to check if module is enabled: {0}")]
    CheckEnabled(#[from] registry::IsModuleEnabledError),
    #[error(transparent)]
    Preinstall(#[from] PreinstallError),
    #[error(transparent)]
    Uninstall(#[from] UninstallError),
    #[error(transparent)]
    Install(#[from] FinishInstallError),
    #[error("failed to re-enable module: {0}")]
    ReEnable(#[from] registry::EnableError),
}

#[derive(Debug, Error)]
pub enum UninstallError {
    #[error("uninstall publication or recovery failed: {0}")]
    Publication(#[from] io::Error),
    #[error("unable to read module manifest file: {0}")]
    Read(#[from] registry::ManifestError),
    #[error(transparent)]
    Disable(#[from] registry::DisableError),
    #[error("unable to remove installed module binary `{0}`: {1}")]
    RemoveBinary(String, #[source] registry::RemoveBinaryError),
    #[error("unable to remove installed module: {0}")]
    RemoveModule(#[from] registry::RemoveModuleError),
}

mod common {
    use super::*;

    #[derive(Debug, Error)]
    pub enum FetchError {
        #[error(transparent)]
        Http(#[from] HttpError),
        #[error("unable to deserialize GitHub API response: {0}")]
        Deserialize(#[from] DeserializeError),
    }

    impl From<reqwest::Error> for FetchError {
        fn from(value: reqwest::Error) -> Self {
            FetchError::Http(HttpError::Http(value))
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
    pub enum FetchChecksumError {
        #[error(transparent)]
        Http(#[from] HttpError),
    }

    impl From<reqwest::Error> for FetchChecksumError {
        fn from(value: reqwest::Error) -> Self {
            FetchChecksumError::Http(HttpError::Http(value))
        }
    }

    #[derive(Debug, Error)]
    pub enum VerifyChecksumError {
        #[error("failed to read target file: {0}")]
        Io(#[from] io::Error),
        #[error("invalid checksum `{0}`, expected `{1}`")]
        InvalidChecksum(String, String),
    }

    #[derive(Debug, Error)]
    pub enum DownloadError {
        #[error(transparent)]
        Http(#[from] HttpError),
        #[error("failed to write data on disk: {0}")]
        Io(#[from] io::Error),
        #[error("no matching asset found")]
        NoMatch,
    }

    impl From<reqwest::Error> for DownloadError {
        fn from(value: reqwest::Error) -> Self {
            DownloadError::Http(HttpError::Http(value))
        }
    }

    #[derive(Debug, Error)]
    pub enum HttpError {
        #[error("HTTP request failed: {0}")]
        Http(#[from] reqwest::Error),
        #[error("HTTP status code was not successful: {0}")]
        NotSuccess(reqwest::StatusCode),
    }

    #[derive(Debug, Error)]
    pub enum PreinstallError {
        #[error(transparent)]
        InvalidModuleName(asimov_module::InvalidModuleName),
        #[error("invalid name for a required module: {0}")]
        InvalidDependencyName(asimov_module::InvalidModuleName),

        #[error("failed fetch release: {0}")]
        FetchRelease(FetchError),

        #[error("no binaries available for platform `{}-{}{}`", .0.os, .0.arch, if let Some(ref libc) = .0.libc { "-".to_string() + libc } else { "".to_string() })]
        NotAvailable(PlatformInfo),

        #[error(transparent)]
        Download(#[from] DownloadError),
        #[error("failed to fetch module manifest: {0}")]
        FetchManifest(FetchError),

        #[error("failed to install dependency module `{0}`: {1}")]
        Dependency(String, Box<InstallError>),

        #[error("failed to fetch checksum: {0}")]
        FetchChecksum(#[from] FetchChecksumError),
        #[error(transparent)]
        VerifyChecksum(#[from] VerifyChecksumError),

        #[error("failed to create directory for extracting: {0}")]
        CreateExtractDir(io::Error),

        #[error("failed to extract archive: {0}")]
        Extract(io::Error),

        #[error("module manifest does not have a choice of model size `{0}`")]
        NoSuchModel(String),

        #[error("error while installing required model: {0}")]
        InstallModel(#[from] asimov_huggingface::HuggingfaceError),
    }

    #[derive(Debug, Error)]
    pub enum WorkDirError {
        #[error("failed to create the module file tree: {0}")]
        CreateFileTree(#[source] registry::CreateFileTreeError),
        #[error("failed to create directory for installing: {0}")]
        CreateDir(#[source] io::Error),
    }

    #[derive(Debug, Error)]
    pub enum FinishInstallError {
        #[error("failed to create directory `{0}`: {1}")]
        CreateDir(PathBuf, #[source] io::Error),
        #[error("failed to write `{0}`: {1}")]
        WriteFile(PathBuf, #[source] io::Error),
        #[error("failed to move binary `{0}` into place: {1}")]
        MoveBinary(String, #[source] io::Error),
        #[cfg(unix)]
        #[error("failed to set permissions: {0}")]
        SetPermissions(#[source] io::Error),
        #[error("failed to serialize module manifest: {0}")]
        Serialize(#[source] serde_json::Error),
        #[error("failed to install module: {0}")]
        AddModule(#[from] registry::AddModuleError),
    }
}
pub use common::*;
