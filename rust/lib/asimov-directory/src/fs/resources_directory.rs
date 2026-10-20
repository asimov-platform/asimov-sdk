// This is free and unencumbered software released into the public domain.

use crate::{InvalidUrlFile, UrlFile, UrlReader, UrlsFile, UrlsReader};
use alloc::{collections::BTreeSet, format, string::String, vec::Vec};
use asimov_core::{
    crates::cap_std::fs::{Dir, DirBuilder, File, Metadata, OpenOptions},
    validate_filename_component,
};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use std::io::{self, ErrorKind};
use tokio::io::{AsyncWriteExt, BufWriter};

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

    /// Opens an async, borrowing reader for the first URL in a `.url` collection.
    ///
    /// Trailing records are ignored; missing collections return `NotFound`.
    /// To read a `.urls` collection this way, wrap [`Self::read_urls`]'s result
    /// in [`UrlReader`]. Both paths use the same confined file-opening checks.
    pub fn read_url(&self, collection: &str) -> io::Result<UrlReader<tokio::fs::File>> {
        self.open_reader(collection, "url").map(UrlReader::from)
    }

    /// Atomically writes a single-URL collection, returning whether it changed.
    ///
    /// An existing file must contain exactly one valid URL; malformed files are
    /// rejected before staging a replacement, including files with extra lines.
    pub async fn write_url(&self, collection: &str, contents: &UrlFile) -> io::Result<bool> {
        match self.read_url(collection) {
            Ok(mut reader) => {
                let existing = reader
                    .next_url()
                    .await?
                    .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, InvalidUrlFile))?;
                let unchanged = existing == contents.url();
                if reader.into_inner().next_url().await?.is_some() {
                    return Err(io::Error::new(ErrorKind::InvalidData, InvalidUrlFile));
                }
                if unchanged {
                    return Ok(false);
                }
            },
            Err(error) if error.kind() == ErrorKind::NotFound => {},
            Err(error) => return Err(error),
        }
        let (directory, name) = self.location(collection, "url", true)?;
        let staged = stage(&directory)?;
        let file = tokio::fs::File::from_std(staged.as_file().try_clone()?.into_std());
        let mut output = BufWriter::new(file);
        output.write_all(contents.url().as_bytes()).await?;
        output.write_all(b"\n").await?;
        output.flush().await?;
        output.get_ref().sync_all().await?;
        drop(output);
        staged.replace(&name)?;
        Ok(true)
    }

    /// Opens an async, borrowing reader for a multi-URL collection.
    ///
    /// A missing collection yields an empty reader. Opening performs synchronous
    /// path and file checks; contents are read asynchronously from that same
    /// confined file handle as [`UrlsReader::next_url`] is called.
    pub fn read_urls(&self, collection: &str) -> io::Result<UrlsReader<tokio::fs::File>> {
        match self.open_reader(collection, "urls") {
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(UrlsReader::empty()),
            result => result,
        }
    }

    /// Appends absent URLs in input order, returning the number added.
    ///
    /// The validated batch is deduplicated against itself and the existing
    /// file. Only the input batch is retained in memory. Existing bytes are
    /// preserved; a no-op does not write the file.
    pub async fn append_urls(&self, collection: &str, urls: &UrlsFile) -> io::Result<usize> {
        validate_collection(collection)?;
        if urls.urls().is_empty() {
            return Ok(0);
        }
        let mut pending: BTreeSet<_> = urls.urls().iter().map(String::as_str).collect();
        let mut reader = self.read_urls(collection)?;
        while let Some(url) = reader.next_url().await? {
            pending.remove(url);
        }
        let added = pending.len();
        if added != 0 {
            let (directory, name) = self.location(collection, "urls", true)?;
            let file = open_file(&directory, &name, true)?;
            let mut output = BufWriter::new(tokio::fs::File::from_std(file.into_std()));
            // The set tests membership; the input batch determines write order.
            for url in urls.urls() {
                if pending.remove(url.as_str()) {
                    output.write_all(url.as_bytes()).await?;
                    output.write_all(b"\n").await?;
                }
            }
            output.flush().await?;
            output.get_ref().sync_all().await?;
        }
        Ok(added)
    }

    /// Atomically removes matching URLs, returning the number removed.
    ///
    /// Surviving URLs retain their original order, spelling, and duplicates.
    /// Absent URLs are ignored. If nothing matches, no file, directory, or
    /// temporary file is created, removed, or rewritten.
    /// Two streaming passes validate the input before staging the replacement;
    /// memory use depends on the removal batch and longest line, not file size.
    pub async fn remove_urls(&self, collection: &str, urls: &UrlsFile) -> io::Result<usize> {
        let targets: BTreeSet<_> = urls.urls().iter().map(String::as_str).collect();
        let mut reader = self.read_urls(collection)?;
        let mut removed = 0;
        while let Some(url) = reader.next_url().await? {
            if targets.contains(url) {
                removed += 1;
            }
        }
        if removed != 0 {
            reader.rewind().await?;
            let (directory, name) = self.location(collection, "urls", false)?;
            let staged = stage(&directory)?;
            let file = tokio::fs::File::from_std(staged.as_file().try_clone()?.into_std());
            let mut output = BufWriter::new(file);
            while let Some(url) = reader.next_url().await? {
                if !targets.contains(url) {
                    output.write_all(url.as_bytes()).await?;
                    output.write_all(b"\n").await?;
                }
            }
            output.flush().await?;
            output.get_ref().sync_all().await?;
            drop(output);
            staged.replace(&name)?;
        }
        Ok(removed)
    }

    fn open_reader(
        &self,
        collection: &str,
        extension: &str,
    ) -> io::Result<UrlsReader<tokio::fs::File>> {
        let (directory, name) = self.location(collection, extension, false)?;
        let file = open_file(&directory, &name, false)?;
        Ok(UrlsReader::new(tokio::fs::File::from_std(file.into_std())))
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

fn stage(directory: &Dir) -> io::Result<cap_tempfile::TempFile<'_>> {
    let staged = cap_tempfile::TempFile::new(directory)?;
    #[cfg(unix)]
    {
        use asimov_core::crates::cap_std::fs::{Permissions, PermissionsExt};
        staged
            .as_file()
            .set_permissions(Permissions::from_mode(0o600))?;
    }
    Ok(staged)
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
