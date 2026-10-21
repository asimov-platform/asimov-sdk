// This is free and unencumbered software released into the public domain.

use crate::{UriFile, UriReader, UrisReader};
use alloc::{format, string::String, vec::Vec};
use asimov_core::{crates::cap_std::fs::Dir, validate_filename_component};
use std::io::{self, ErrorKind, Write};

/// Remote `.uri` files accessed relative to a [`super::StateDirectory`].
///
/// Obtain this through [`super::StateDirectory::remotes`]. Names must be
/// portable filename components; storage appends `.uri` to each name. Symlinks
/// and non-regular remote files are rejected; directory-relative access confines
/// resolution to the state root. Files follow [`UriFile`]'s IRI/LF format;
/// HTTP(S) validation belongs to `asimov-remote`.
#[derive(Debug)]
pub struct RemotesDirectory {
    root: Dir,
}

impl RemotesDirectory {
    pub(super) fn new(root: Dir) -> Self {
        Self { root }
    }

    /// Lists names without their `.uri` storage suffix in lexical order.
    ///
    /// Absent storage is empty. Subdirectories (including temporary staging
    /// directories) and files without the `.uri` extension are ignored.
    pub fn names(&self) -> io::Result<Vec<String>> {
        let directory = match self.directory(false) {
            Ok(directory) => directory,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut names = Vec::new();
        for entry in directory.entries()? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                continue;
            }
            let filename = entry
                .file_name()
                .into_string()
                .map_err(|_| io::Error::new(ErrorKind::InvalidData, "remote name is not UTF-8"))?;
            let Some(name) = filename.strip_suffix(".uri") else {
                continue;
            };
            validate_filename_component(name)
                .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
            require_file(&directory, &filename)?;
            names.push(String::from(name));
        }
        names.sort();
        Ok(names)
    }

    /// Opens an async, borrowing reader for the first IRI in a remote's file.
    ///
    /// Opening checks the path synchronously; contents are read asynchronously
    /// through [`UriReader`]. Missing files return `NotFound`.
    pub fn read(&self, name: &str) -> io::Result<UriReader<tokio::fs::File>> {
        let filename = filename(name)?;
        let directory = self.directory(false)?;
        require_file(&directory, &filename)?;
        let file = directory.open(&filename)?;
        Ok(UriReader::from(UrisReader::new(tokio::fs::File::from_std(
            file.into_std(),
        ))))
    }

    /// Creates a remote containing one absolute IRI followed by LF.
    ///
    /// Publishes the complete file without replacing an existing name, even
    /// during concurrent additions. Invalid input never creates remote storage.
    pub fn create(&self, name: &str, value: &str) -> io::Result<()> {
        let filename = filename(name)?;
        let contents =
            UriFile::new(value).map_err(|error| io::Error::new(ErrorKind::InvalidInput, error))?;
        let directory = self.directory(true)?;
        let staged = cap_tempfile::tempdir_in(&directory)?;
        let mut file = staged.create("remote.uri")?;
        #[cfg(unix)]
        {
            use asimov_core::crates::cap_std::fs::{Permissions, PermissionsExt};
            file.set_permissions(Permissions::from_mode(0o600))?;
        }
        write!(file, "{contents}")?;
        file.sync_all()?;
        staged.hard_link("remote.uri", &directory, &filename)
    }

    /// Renames a remote without replacing an existing destination.
    ///
    /// Links the new name before removing the old one. If removal fails or the
    /// process is interrupted between those steps, both names may remain.
    /// Renaming to the same name succeeds only if the remote exists.
    pub fn rename(&self, old: &str, new: &str) -> io::Result<()> {
        let old = filename(old)?;
        let new = filename(new)?;
        let directory = self.directory(false)?;
        require_file(&directory, &old)?;
        if old == new {
            return Ok(());
        }
        directory.hard_link(&old, &directory, &new)?;
        directory.remove_file(old)
    }

    /// Removes a remote, returning `NotFound` if it does not exist.
    pub fn remove(&self, name: &str) -> io::Result<()> {
        let filename = filename(name)?;
        let directory = self.directory(false)?;
        require_file(&directory, &filename)?;
        directory.remove_file(filename)
    }

    fn directory(&self, create: bool) -> io::Result<Dir> {
        if create {
            let builder = &mut asimov_core::crates::cap_std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use asimov_core::crates::cap_std::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match self.root.create_dir_with("remotes", builder) {
                Ok(()) => {},
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {},
                Err(error) => return Err(error),
            }
        }
        if !self.root.symlink_metadata("remotes")?.is_dir() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "remotes must be a directory, not a file or symlink",
            ));
        }
        self.root.open_dir("remotes")
    }
}

fn filename(name: &str) -> io::Result<String> {
    validate_filename_component(name)
        .map_err(|error| io::Error::new(ErrorKind::InvalidInput, error))?;
    Ok(format!("{name}.uri"))
}

fn require_file(directory: &Dir, name: &str) -> io::Result<()> {
    if !directory.symlink_metadata(name)?.is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("remote {name:?} must be a regular file, not a symlink or directory"),
        ));
    }
    Ok(())
}
