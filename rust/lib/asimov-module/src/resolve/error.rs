// This is free and unencumbered software released into the public domain.

use alloc::string::String;

#[cfg(feature = "std")]
#[derive(Debug, thiserror::Error)]
pub enum FromDirError {
    #[error("failed to read manifest directory `{path}`: {source}")]
    ManifestDirIo {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read manifest file `{path}`: {source}")]
    ManifestIo {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[cfg(feature = "yaml")]
    #[error("failed to parse manifest file `{path}`: {source}")]
    Parse {
        path: std::path::PathBuf,
        #[source]
        source: serde_yaml_ng::Error,
    },
    #[cfg(feature = "json")]
    #[error("failed to parse JSON manifest file `{path}`: {source}")]
    ParseJson {
        path: std::path::PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to add manifest file `{path}` to resolver: {source}")]
    Insert {
        path: std::path::PathBuf,
        #[source]
        source: InsertManifestError,
    },
}

#[cfg(all(feature = "cli", feature = "std"))]
impl From<FromDirError> for clientele::SysexitsError {
    fn from(value: FromDirError) -> Self {
        use FromDirError::*;
        use clientele::SysexitsError::*;
        match value {
            ManifestDirIo { .. } => EX_IOERR,
            ManifestIo { .. } => EX_IOERR,
            #[cfg(feature = "yaml")]
            Parse { .. } => EX_CONFIG,
            #[cfg(feature = "json")]
            ParseJson { .. } => EX_CONFIG,
            Insert { source, .. } => source.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InsertManifestError {
    #[error("invalid url: {0}")]
    Url(#[from] UrlParseError),
    #[error("invalid content type: {0}")]
    ContentType(#[from] mime::FromStrError),
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum UrlParseError {
    #[error("URL can't be empty")]
    EmptyUrl,
    #[error("invalid URL `{url}`: {source}")]
    InvalidUrl {
        url: String,
        #[source]
        source: url::ParseError,
    },
}

#[cfg(all(feature = "cli", feature = "std"))]
impl From<InsertManifestError> for clientele::SysexitsError {
    fn from(_value: InsertManifestError) -> Self {
        clientele::SysexitsError::EX_USAGE
    }
}
