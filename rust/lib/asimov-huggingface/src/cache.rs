// This is free and unencumbered software released into the public domain.

use crate::{HuggingfaceError as Error, Result};
use alloc::string::ToString as _;
use hf_hub::{Cache, Repo, RepoType};
use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

pub fn validate_path(path: &str) -> Result<()> {
    if path
        .split('/')
        .any(|part| asimov_core::validate_filename_component(part).is_err())
    {
        return Err(Error::InvalidInput("expected a portable relative path"));
    }
    Ok(())
}

pub fn validate_commit(commit: &str) -> Result<()> {
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::InvalidResponse("invalid commit hash"));
    }
    Ok(())
}

pub fn repo(id: &str, revision: &str) -> Result<Repo> {
    validate_path(id)?;
    validate_path(revision)?;
    Ok(Repo::with_revision(
        id.into(),
        RepoType::Model,
        revision.into(),
    ))
}

pub fn root(cache: &Cache, repo: &Repo) -> PathBuf {
    cache.path().join(repo.folder_name())
}

pub fn snapshot(cache: &Cache, repo: &Repo, commit: &str) -> PathBuf {
    root(cache, repo).join("snapshots").join(commit)
}

pub fn get(cache: &Cache, repo: &Repo, filename: &str) -> Option<PathBuf> {
    let commit = if validate_commit(repo.revision()).is_ok() {
        repo.revision().to_string()
    } else {
        fs::read_to_string(root(cache, repo).join("refs").join(repo.revision())).ok()?
    };
    let commit = commit.trim();
    validate_commit(commit).ok()?;
    let path = snapshot(cache, repo, commit).join(filename);
    path.is_file().then_some(path)
}

pub fn publish_ref(cache: &Cache, repo: &Repo, commit: &str) -> Result<()> {
    let path = root(cache, repo).join("refs").join(repo.revision());
    let parent = path.parent().expect("ref has parent directory");
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(commit.as_bytes())?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub fn link_blob(blob: &Path, pointer: &Path, filename: &str) -> Result<()> {
    if pointer.is_file() {
        return Ok(());
    }
    fs::create_dir_all(pointer.parent().expect("snapshot file has a parent"))?;
    #[cfg(unix)]
    let result = {
        let mut relative = PathBuf::new();
        for _ in 0..=filename.split('/').count() {
            relative.push("..");
        }
        relative.push("blobs");
        relative.push(blob.file_name().expect("blob has a name"));
        std::os::unix::fs::symlink(relative, pointer)
    };
    #[cfg(not(unix))]
    let result = {
        let _ = filename;
        // Hard links need no Windows symlink privilege. Copy atomically on
        // filesystems where links are unavailable, retaining the shared blob.
        fs::hard_link(blob, pointer).or_else(|_| {
            let mut copy = tempfile::NamedTempFile::new_in(pointer.parent().unwrap())?;
            std::io::copy(&mut fs::File::open(blob)?, &mut copy)?;
            copy.persist(pointer).map_err(|error| error.error)?;
            Ok::<_, std::io::Error>(())
        })
    };
    match result {
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && pointer.is_file() => {
            Ok(())
        },
        result => Ok(result?),
    }
}
