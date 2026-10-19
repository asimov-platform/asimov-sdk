// This is free and unencumbered software released into the public domain.

use crate::{HuggingfaceError as Error, Progress, Result, cache, http::Client};
use alloc::{
    format,
    string::{String, ToString as _},
};
use hf_hub::{
    Cache, Repo,
    api::{Progress as _, sync::ApiError},
};
use reqwest::{StatusCode, header::HeaderMap};
use std::{
    fs::{self, OpenOptions},
    io::{Seek as _, SeekFrom, Write as _},
    path::PathBuf,
};

struct Metadata {
    commit: String,
    etag: String,
    size: u64,
}

fn header<'a>(headers: &'a HeaderMap, name: &'static str) -> Result<&'a str> {
    headers
        .get(name)
        .ok_or(ApiError::MissingHeader(name))?
        .to_str()
        .map_err(|_| ApiError::InvalidHeader(name).into())
}

fn range(headers: &HeaderMap) -> Result<(u64, u64, u64)> {
    let parse = || {
        let value = headers
            .get("content-range")?
            .to_str()
            .ok()?
            .strip_prefix("bytes ")?;
        let (span, total) = value.split_once('/')?;
        let (start, end) = span.split_once('-')?;
        let (start, end, total) = (start.parse().ok()?, end.parse().ok()?, total.parse().ok()?);
        (start <= end && end < total).then_some((start, end, total))
    };
    parse().ok_or(Error::InvalidResponse("invalid Content-Range"))
}

async fn metadata(client: &mut Client, repo: &Repo, filename: &str) -> Result<Metadata> {
    let mut values = HeaderMap::new();
    let origin = client.endpoint.origin();
    let response = client
        .get(
            client.url(repo, Some(filename)),
            Some("bytes=0-0"),
            |url, headers| {
                // Content IDs belong to the Hub, not a redirected storage provider.
                if url.origin() == origin {
                    for name in ["x-repo-commit", "x-linked-etag", "etag"] {
                        if let Some(value) = headers.get(name) {
                            values.insert(name, value.clone());
                        }
                    }
                }
                Ok(())
            },
        )
        .await?;
    let size = match response.status() {
        StatusCode::PARTIAL_CONTENT => {
            let (start, end, size) = range(response.headers())?;
            if start != 0 || end != 0 {
                return Err(Error::InvalidResponse("metadata range must be bytes 0-0"));
            }
            size
        },
        StatusCode::OK => response
            .content_length()
            .ok_or(ApiError::MissingHeader("content-length"))?,
        StatusCode::RANGE_NOT_SATISFIABLE
            if header(response.headers(), "content-range")? == "bytes */0" =>
        {
            0
        },
        _ => {
            response.error_for_status()?;
            return Err(Error::InvalidResponse("unexpected metadata status"));
        },
    };
    identity_encoding(response.headers())?;
    let commit = header(&values, "x-repo-commit")?.to_string();
    cache::validate_commit(&commit)?;
    let etag = header(
        &values,
        if values.contains_key("x-linked-etag") {
            "x-linked-etag"
        } else {
            "etag"
        },
    )?
    .trim_matches('"')
    .to_string();
    // Hub ETags are Git blob SHA-1 or LFS SHA-256 content IDs, used as filenames.
    cache::validate_commit(&etag)
        .map_err(|_| Error::InvalidResponse("invalid ETag content hash"))?;
    Ok(Metadata { commit, etag, size })
}

fn identity_encoding(headers: &HeaderMap) -> Result<()> {
    if headers
        .get("content-encoding")
        .is_some_and(|value| !value.as_bytes().eq_ignore_ascii_case(b"identity"))
    {
        return Err(Error::InvalidResponse("encoded response cannot be resumed"));
    }
    Ok(())
}

pub async fn file(
    client: &mut Client,
    cache: &Cache,
    repo: &Repo,
    filename: &str,
    expected_commit: Option<&str>,
) -> Result<PathBuf> {
    let metadata = metadata(client, repo, filename).await?;
    if expected_commit.is_some_and(|commit| commit != metadata.commit) {
        return Err(Error::InvalidResponse(
            "file metadata changed the pinned commit",
        ));
    }
    let blob = cache::root(cache, repo).join("blobs").join(&metadata.etag);
    fs::create_dir_all(blob.parent().unwrap())?;
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(blob.with_extension("lock"))?;
    let mut attempts = 0;
    loop {
        match lock.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if attempts < 5 => {
                attempts += 1;
                tokio::time::sleep(core::time::Duration::from_secs(1)).await;
            },
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(ApiError::LockAcquisition(blob.with_extension("lock")).into());
            },
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
    if !fs::metadata(&blob).is_ok_and(|file| file.is_file() && file.len() == metadata.size) {
        let part = blob.with_extension("part");
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&part)?;
        let mut written = file.metadata()?.len();
        if written > metadata.size {
            return Err(ApiError::InvalidResume.into());
        }
        let mut progress = Progress::new();
        progress.init_bytes(metadata.size, written);
        if written < metadata.size {
            // Pin the body request too: a branch may move after its probe.
            let pinned = cache::repo(&repo.url(), &metadata.commit)?;
            let mut response = client
                .get(
                    client.url(&pinned, Some(filename)),
                    Some(&format!("bytes={written}-")),
                    |_, _| Ok(()),
                )
                .await?
                .error_for_status()?;
            identity_encoding(response.headers())?;
            match response.status() {
                StatusCode::PARTIAL_CONTENT => {
                    if range(response.headers())? != (written, metadata.size - 1, metadata.size) {
                        return Err(Error::InvalidResponse(
                            "download range does not match metadata",
                        ));
                    }
                },
                StatusCode::OK => {
                    // A server may ignore Range. Restart instead of appending
                    // a complete representation to the partial file.
                    file.set_len(0)?;
                    written = 0;
                    progress.init_bytes(metadata.size, 0);
                },
                _ => return Err(Error::InvalidResponse("unexpected download status")),
            }
            file.seek(SeekFrom::Start(written))?;
            while let Some(chunk) = response.chunk().await? {
                if chunk.len() as u64 > metadata.size - written {
                    return Err(Error::InvalidResponse("download exceeded metadata size"));
                }
                file.write_all(&chunk)?;
                written += chunk.len() as u64;
                progress.update(chunk.len());
            }
            if written != metadata.size {
                return Err(Error::InvalidResponse("incomplete download"));
            }
        }
        file.sync_all()?;
        drop(file);
        fs::rename(part, &blob)?;
        progress.finish();
    }
    let pointer = cache::snapshot(cache, repo, &metadata.commit).join(filename);
    cache::link_blob(&blob, &pointer, filename)?;
    cache::publish_ref(cache, repo, &metadata.commit)?;
    drop(lock);
    Ok(pointer)
}
