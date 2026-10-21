// This is free and unencumbered software released into the public domain.

use alloc::{format, string::String, vec::Vec};
use asimov_core::{crates::cap_std::fs::Dir, validate_filename_component};
use std::io::{self, ErrorKind, Read, Write};

/// Plain-text remote files accessed relative to a [`super::StateDirectory`].
///
/// Obtain this through [`super::StateDirectory::remotes`]. Names must be
/// portable filename components. Symlinks and non-regular remote files are
/// rejected; directory-relative access confines resolution to the state root.
/// URL validation belongs to `asimov-remote`.
#[derive(Debug)]
pub struct RemotesDirectory {
    root: Dir,
}

impl RemotesDirectory {
    pub(super) fn new(root: Dir) -> Self {
        Self { root }
    }

    /// Lists remote names in lexical order; absent storage is an empty list.
    ///
    /// Subdirectories (including temporary staging directories) are ignored.
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
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| io::Error::new(ErrorKind::InvalidData, "remote name is not UTF-8"))?;
            validate_filename_component(&name)
                .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
            require_file(&directory, &name)?;
            names.push(name);
        }
        names.sort();
        Ok(names)
    }

    /// Reads a remote's exact file contents, including its terminating newline.
    pub fn read(&self, name: &str) -> io::Result<String> {
        validate_name(name)?;
        let directory = self.directory(false)?;
        require_file(&directory, name)?;
        let mut file = directory.open(name)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(contents)
    }

    /// Creates a remote containing one nonempty line, appending a newline.
    ///
    /// Publishes the complete file without replacing an existing name, even
    /// during concurrent additions. Invalid input never creates remote storage.
    pub fn create(&self, name: &str, value: &str) -> io::Result<()> {
        validate_name(name)?;
        if value.is_empty() || value.contains(['\r', '\n']) {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "remote value must be one nonempty line",
            ));
        }
        let directory = self.directory(true)?;
        let staged = cap_tempfile::tempdir_in(&directory)?;
        let mut file = staged.create("url")?;
        #[cfg(unix)]
        {
            use asimov_core::crates::cap_std::fs::{Permissions, PermissionsExt};
            file.set_permissions(Permissions::from_mode(0o600))?;
        }
        writeln!(file, "{value}")?;
        file.sync_all()?;
        staged.hard_link("url", &directory, name)
    }

    /// Renames a remote without replacing an existing destination.
    ///
    /// Links the new name before removing the old one. If removal fails or the
    /// process is interrupted between those steps, both names may remain.
    /// Renaming to the same name succeeds only if the remote exists.
    pub fn rename(&self, old: &str, new: &str) -> io::Result<()> {
        validate_name(old)?;
        validate_name(new)?;
        let directory = self.directory(false)?;
        require_file(&directory, old)?;
        if old == new {
            return Ok(());
        }
        directory.hard_link(old, &directory, new)?;
        directory.remove_file(old)
    }

    /// Removes a remote, returning `NotFound` if it does not exist.
    pub fn remove(&self, name: &str) -> io::Result<()> {
        validate_name(name)?;
        let directory = self.directory(false)?;
        require_file(&directory, name)?;
        directory.remove_file(name)
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

fn validate_name(name: &str) -> io::Result<()> {
    validate_filename_component(name)
        .map_err(|error| io::Error::new(ErrorKind::InvalidInput, error))
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
