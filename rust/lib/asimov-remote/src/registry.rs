// This is free and unencumbered software released into the public domain.

use alloc::{format, string::String, vec::Vec};
use asimov_directory::fs::{RemotesDirectory, StateDirectory};
use std::io::{self, ErrorKind};

/// A tracked remote and its URL, preserving the URL's original spelling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Remote {
    /// The remote's name, stored as `remotes/NAME.uri` in the state directory.
    pub name: String,
    /// An absolute HTTP(S) URL without a terminating newline.
    pub url: String,
}

/// A registry of named remotes backed exclusively by a [`StateDirectory`].
///
/// Files use the `.uri` extension and contain exactly one HTTP(S) IRI followed
/// by LF. Reads use [`asimov_directory::UriReader`] and require a Tokio runtime.
/// Invalid names or URLs return `InvalidInput`; malformed stored URLs return
/// `InvalidData`.
/// Existing destinations return `AlreadyExists`, and absent remotes return
/// `NotFound`. Other filesystem errors retain their original kind.
#[derive(Debug)]
pub struct RemotesRegistry {
    directory: RemotesDirectory,
}

impl RemotesRegistry {
    /// Opens remote storage without creating the `remotes/` subdirectory.
    pub fn open(state: &StateDirectory) -> io::Result<Self> {
        Ok(Self {
            directory: state.remotes()?,
        })
    }

    /// Lists remotes in name order, validating every stored URL before return.
    /// An absent `remotes/` directory is an empty list.
    pub async fn list(&self) -> io::Result<Vec<Remote>> {
        let names = self.directory.names()?;
        let mut remotes = Vec::with_capacity(names.len());
        for name in names {
            remotes.push(self.get(&name).await?);
        }
        Ok(remotes)
    }

    /// Reads and validates a tracked remote.
    pub async fn get(&self, name: &str) -> io::Result<Remote> {
        let mut reader = self.directory.read(name)?;
        let url = reader.next_uri().await?.ok_or_else(|| {
            io::Error::new(ErrorKind::InvalidData, format!("remote {name:?} is empty"))
        })?;
        validate_url(url).map_err(|error| {
            io::Error::new(ErrorKind::InvalidData, format!("remote {name:?}: {error}"))
        })?;
        let url = String::from(url);
        if reader.into_inner().next_uri().await?.is_some() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("remote {name:?} contains more than one IRI"),
            ));
        }
        Ok(Remote {
            name: name.into(),
            url,
        })
    }

    /// Adds a remote without replacing an existing name or normalizing its URL.
    pub fn add(&self, name: &str, url: &str) -> io::Result<()> {
        validate_url(url)?;
        self.directory.create(name, url)
    }

    /// Renames a remote without replacing an existing destination.
    /// See [`RemotesDirectory::rename`] for interruption behavior.
    pub fn rename(&self, old: &str, new: &str) -> io::Result<()> {
        self.directory.rename(old, new)
    }

    /// Removes a remote, including one with a malformed stored URL.
    pub fn remove(&self, name: &str) -> io::Result<()> {
        self.directory.remove(name)
    }
}

fn validate_url(value: &str) -> io::Result<()> {
    // The URL parser otherwise strips control characters and repairs missing
    // authority separators, which would conceal malformed plain-text records.
    if value
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
        || !value
            .split_once("://")
            .is_some_and(|(_, rest)| !rest.is_empty() && !rest.starts_with('/'))
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "remote URL must be an absolute HTTP(S) URL without whitespace or backslashes",
        ));
    }
    let url = url::Url::parse(value).map_err(|error| {
        io::Error::new(
            ErrorKind::InvalidInput,
            format!("invalid remote URL: {error}"),
        )
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "remote URL must use the http or https scheme and include a host",
        ));
    }
    Ok(())
}
