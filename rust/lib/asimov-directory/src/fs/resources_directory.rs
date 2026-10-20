// This is free and unencumbered software released into the public domain.

use crate::{UrlFile, UrlsFile};
use alloc::{format, string::String, vec::Vec};
use asimov_core::{
    crates::cap_std::fs::{Dir, DirBuilder, File, Metadata, OpenOptions},
    validate_filename_component,
};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use core::fmt::Display;
use std::io::{self, ErrorKind, Read, Write};

/// Named `.url` and `.urls` collections under a state directory's `resources/`.
///
/// Obtain this through [`super::StateDirectory::resources`]. Collection names
/// omit the extension and may contain `/`-separated portable components, e.g.
/// `my/x/tweets`. All access uses directory handles and rejects symlinks,
/// non-regular files, and (on Unix) multiply-linked files. Reads and no-op
/// removals do not create storage. Mutating callers must serialize writers.
#[derive(Debug)]
pub struct ResourcesDirectory {
    root: Dir,
}

impl ResourcesDirectory {
    /// The collection used when none is specified by the application.
    pub const DEFAULT_COLLECTION: &str = "tracked";

    pub(super) fn new(root: Dir) -> Self {
        Self { root }
    }

    /// Lists relative `.url`/`.urls` filenames recursively in lexical order.
    ///
    /// Missing storage is empty. Unsupported file extensions are ignored;
    /// symlinks are rejected, including directory symlinks and dangling links.
    pub fn files(&self) -> io::Result<Vec<String>> {
        let directory = match self.directory(false) {
            Ok(directory) => directory,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut files = Vec::new();
        collect_files(&directory, "", &mut files)?;
        files.sort();
        Ok(files)
    }

    /// Reads a single-URL collection, requiring exactly one terminated URL.
    pub fn read_url(&self, collection: &str) -> io::Result<UrlFile> {
        let (directory, name) = self.location(collection, "url", false)?;
        read_contents(&mut open_file(&directory, &name, false)?)?
            .parse()
            .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
    }

    /// Atomically writes a single-URL collection, returning whether it changed.
    pub fn write_url(&self, collection: &str, contents: &UrlFile) -> io::Result<bool> {
        match self.read_url(collection) {
            Ok(existing) if existing == *contents => return Ok(false),
            Ok(_) => {},
            Err(error) if error.kind() == ErrorKind::NotFound => {},
            Err(error) => return Err(error),
        }
        let (directory, name) = self.location(collection, "url", true)?;
        replace(&directory, &name, contents)?;
        Ok(true)
    }

    /// Reads a multi-URL collection; a missing collection is empty.
    pub fn read_urls(&self, collection: &str) -> io::Result<UrlsFile> {
        let result = (|| {
            let (directory, name) = self.location(collection, "urls", false)?;
            read_contents(&mut open_file(&directory, &name, false)?)?
                .parse()
                .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
        })();
        match result {
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(UrlsFile::default()),
            result => result,
        }
    }

    /// Appends absent URLs in input order, returning the number added.
    ///
    /// The validated batch is deduplicated against itself and the existing
    /// file. Existing bytes are preserved; a no-op does not write the file.
    pub fn append_urls(&self, collection: &str, urls: &UrlsFile) -> io::Result<usize> {
        validate_collection(collection)?;
        if urls.urls().is_empty() {
            return Ok(0);
        }
        let mut existing = self.read_urls(collection)?;
        let before = existing.urls().len();
        let added = existing.append(urls);
        if added != 0 {
            let (directory, name) = self.location(collection, "urls", true)?;
            let mut file = open_file(&directory, &name, true)?;
            let mut suffix = String::new();
            for url in &existing.urls()[before..] {
                suffix.push_str(url);
                suffix.push('\n');
            }
            file.write_all(suffix.as_bytes())?;
            file.sync_all()?;
        }
        Ok(added)
    }

    /// Atomically removes matching URLs, returning the number removed.
    ///
    /// Absent URLs are ignored. If nothing matches, no file, directory, or
    /// temporary file is created, removed, or rewritten.
    pub fn remove_urls(&self, collection: &str, urls: &UrlsFile) -> io::Result<usize> {
        let mut existing = self.read_urls(collection)?;
        let removed = existing.remove(urls);
        if removed != 0 {
            let (directory, name) = self.location(collection, "urls", false)?;
            replace(&directory, &name, &existing)?;
        }
        Ok(removed)
    }

    fn directory(&self, create: bool) -> io::Result<Dir> {
        open_directory(&self.root, "resources", create)
    }

    fn location(
        &self,
        collection: &str,
        extension: &str,
        create: bool,
    ) -> io::Result<(Dir, String)> {
        validate_collection(collection)?;
        let mut directory = self.directory(create)?;
        let mut components = collection.split('/').peekable();
        while let Some(component) = components.next() {
            if components.peek().is_none() {
                let other = if extension == "urls" { "url" } else { "urls" };
                match directory.symlink_metadata(format!("{component}.{other}")) {
                    Err(error) if error.kind() == ErrorKind::NotFound => {},
                    Err(error) => return Err(error),
                    Ok(_) => {
                        return Err(io::Error::new(
                            ErrorKind::InvalidData,
                            "collection already uses the other URL file extension",
                        ));
                    },
                }
                return Ok((directory, format!("{component}.{extension}")));
            }
            directory = open_directory(&directory, component, create)?;
        }
        unreachable!("validated collection has at least one component")
    }
}

fn validate_collection(collection: &str) -> io::Result<()> {
    for component in collection.split('/') {
        validate_filename_component(component)
            .map_err(|error| io::Error::new(ErrorKind::InvalidInput, error))?;
    }
    Ok(())
}

fn open_directory(parent: &Dir, name: &str, create: bool) -> io::Result<Dir> {
    if create {
        let builder = &mut DirBuilder::new();
        #[cfg(unix)]
        {
            use asimov_core::crates::cap_std::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match parent.create_dir_with(name, builder) {
            Ok(()) => {},
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {},
            Err(error) => return Err(error),
        }
    }
    parent.open_dir_nofollow(name)
}

fn require_file(metadata: &Metadata) -> io::Result<()> {
    let regular = metadata.is_file();
    #[cfg(unix)]
    let regular = {
        use asimov_core::crates::cap_std::fs::MetadataExt;
        regular && metadata.nlink() == 1
    };
    if !regular {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "resource collection must be a regular file without symlinks or hard links",
        ));
    }
    Ok(())
}

fn open_file(directory: &Dir, name: &str, append: bool) -> io::Result<File> {
    match directory.symlink_metadata(name) {
        Ok(metadata) => require_file(&metadata)?,
        Err(error) if append && error.kind() == ErrorKind::NotFound => {},
        Err(error) => return Err(error),
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .append(append)
        .create(append)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use asimov_core::crates::cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = directory.open_with(name, &options)?;
    require_file(&file.metadata()?)?;
    Ok(file)
}

fn read_contents(file: &mut File) -> io::Result<String> {
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents)
}

fn replace(directory: &Dir, name: &str, contents: &impl Display) -> io::Result<()> {
    let mut staged = cap_tempfile::TempFile::new(directory)?;
    #[cfg(unix)]
    {
        use asimov_core::crates::cap_std::fs::{Permissions, PermissionsExt};
        staged
            .as_file()
            .set_permissions(Permissions::from_mode(0o600))?;
    }
    write!(staged, "{contents}")?;
    staged.as_file().sync_all()?;
    staged.replace(name)
}

fn collect_files(directory: &Dir, prefix: &str, files: &mut Vec<String>) -> io::Result<()> {
    for entry in directory.entries()? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| io::Error::new(ErrorKind::InvalidData, "resource name is not UTF-8"))?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "resource storage must not contain symlinks",
            ));
        }
        if file_type.is_dir() {
            validate_collection(&name)?;
            collect_files(
                &directory.open_dir_nofollow(&name)?,
                &format!("{prefix}{name}/"),
                files,
            )?;
        } else if let Some(stem) = name
            .strip_suffix(".urls")
            .or_else(|| name.strip_suffix(".url"))
        {
            validate_collection(stem)?;
            require_file(&directory.symlink_metadata(&name)?)?;
            files.push(format!("{prefix}{name}"));
        }
    }
    Ok(())
}
