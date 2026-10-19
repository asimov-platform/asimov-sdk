// This is free and unencumbered software released into the public domain.

//! Cache-aware model downloads using shared per-origin HTTP proxy policy.

use crate::{HuggingfaceError, Result, cache, download, http::Client};
use core::future::Future;
use hf_hub::{Cache, api::RepoInfo};
use std::path::PathBuf;

/// Finds a file cached for the model's `main` revision, honoring `HF_HOME`.
pub fn file_exists(repo: &str, filename: &str) -> Option<PathBuf> {
    let repo = cache::repo(repo, "main").ok()?;
    cache::validate_path(filename).ok()?;
    cache::get(&Cache::from_env(), &repo, filename)
}

/// Ensures a model file is cached, with a progress bar for missing content.
///
/// Honors `HF_HOME`, `HF_ENDPOINT`, and the saved token in `HF_HOME/token`.
/// Metadata, downloads, and redirects use `asimov-http` proxy discovery. The
/// token is sent only to the configured endpoint's origin. Requests have
/// 10-second connection and 30-second read timeouts, with ten redirects maximum.
/// Each operation pools clients with a proxy snapshot per origin.
/// Interrupted downloads retain a resumable `.part` file.
///
/// This is a blocking API. Its HTTP runtime runs on a separate thread, allowing
/// calls from within an existing async runtime. Cache hits do not access HTTP.
///
/// # Errors
///
/// Returns input, proxy, HTTP, metadata, or cache I/O errors. Incomplete or
/// inconsistent responses are not published as cached files.
pub fn ensure_file(repo: &str, filename: &str) -> Result<PathBuf> {
    let repo = cache::repo(repo, "main")?;
    cache::validate_path(filename)?;
    let cache = Cache::from_env();
    if let Some(path) = cache::get(&cache, &repo, filename) {
        return Ok(path);
    }
    run(async move {
        let mut client = Client::new(&cache)?;
        download::file(&mut client, &cache, &repo, filename, None).await
    })
}

/// Ensures a complete model snapshot is cached and returns its root directory.
///
/// Resolves `revision` (default: `main`) to one commit before downloading files.
/// Cache lookups and downloads use that commit throughout, including nested
/// files. Uses the transport, authentication, and blocking policy of
/// [`ensure_file`].
///
/// # Errors
///
/// Returns [`HuggingfaceError::EmptySnapshot`] for an empty repository, or the
/// input, proxy, HTTP, metadata, and cache I/O errors described by [`ensure_file`].
pub fn ensure_snapshot(repo: &str, revision: Option<&str>) -> Result<PathBuf> {
    let repo = cache::repo(repo, revision.unwrap_or("main"))?;
    let cache = Cache::from_env();
    run(async move {
        let mut client = Client::new(&cache)?;
        let info: RepoInfo = client
            .get(client.url(&repo, None), None, |_, _| Ok(()))
            .await?
            .error_for_status()?
            .json()
            .await?;
        cache::validate_commit(&info.sha)?;
        if info.siblings.is_empty() {
            return Err(HuggingfaceError::EmptySnapshot);
        }
        for sibling in &info.siblings {
            cache::validate_path(&sibling.rfilename)?;
        }
        let pinned = cache::repo(&repo.url(), &info.sha)?;
        for sibling in info.siblings {
            if cache::get(&cache, &pinned, &sibling.rfilename).is_none() {
                download::file(
                    &mut client,
                    &cache,
                    &pinned,
                    &sibling.rfilename,
                    Some(&info.sha),
                )
                .await?;
            }
        }
        cache::publish_ref(&cache, &repo, &info.sha)?;
        Ok(cache::snapshot(&cache, &repo, &info.sha))
    })
}

fn run<F>(future: F) -> Result<PathBuf>
where
    F: Future<Output = Result<PathBuf>> + Send,
{
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("asimov-huggingface".into())
            .spawn_scoped(scope, move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?
                    .block_on(future)
            })?
            .join()
            .map_err(|_| HuggingfaceError::Worker)?
    })
}
