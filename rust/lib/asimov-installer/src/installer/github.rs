// This is free and unencumbered software released into the public domain.

use super::error::{DownloadError, FetchChecksumError, FetchError, HttpError, VerifyChecksumError};
use alloc::{
    borrow::ToOwned as _,
    format,
    string::{String, ToString as _},
    vec,
};
use asimov_module::ModuleManifest;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

#[derive(Debug, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
}

async fn fetch_release_tag(client: &reqwest::Client, url: &str) -> Result<String, FetchError> {
    let response = client.get(url).send().await.inspect_err(|_err| {
        #[cfg(feature = "tracing")]
        tracing::debug!(err = ?_err);
    })?;

    if !response.status().is_success() {
        return Err(HttpError::NotSuccess(response.status()).into());
    }

    let content = response.text().await.inspect_err(|_err| {
        #[cfg(feature = "tracing")]
        tracing::debug!(err = ?_err);
    })?;

    serde_json::from_str::<GitHubRelease>(&content)
        .inspect_err(|_err| {
            #[cfg(feature = "tracing")]
            tracing::debug!(err = ?_err, ?content);
        })
        .map_err(|e| FetchError::Deserialize(e.into()))
        .map(|release| release.tag_name)
}

#[cfg_attr(feature = "tracing", tracing::instrument(skip_all))]
pub async fn fetch_latest_release(
    client: &reqwest::Client,
    module_name: impl AsRef<str>,
) -> Result<String, FetchError> {
    async fn by_api(
        client: &reqwest::Client,
        module_name: impl AsRef<str>,
    ) -> Result<String, FetchError> {
        let url = format!(
            "https://api.github.com/repos/asimov-modules/asimov-{}-module/releases/latest",
            module_name.as_ref()
        );

        fetch_release_tag(client, &url).await
    }

    async fn by_redirect(
        client: &reqwest::Client,
        module_name: impl AsRef<str>,
    ) -> Result<String, FetchError> {
        let url = format!(
            "https://github.com/asimov-modules/asimov-{}-module/releases/latest",
            module_name.as_ref()
        );

        let response = client.head(&url).send().await.inspect_err(|_err| {
            #[cfg(feature = "tracing")]
            tracing::debug!(err = ?_err);
        })?;

        let final_url = response.url().as_str();
        if final_url == url {
            // fallback to trying through the API
            return by_api(client, &module_name).await;
        }
        #[cfg(feature = "tracing")]
        tracing::debug!("got redirected to: {final_url}");

        let mut parts = final_url.split('/');

        Ok(parts.next_back().unwrap().into())
    }

    by_redirect(client, &module_name).await
}

#[cfg_attr(feature = "tracing", tracing::instrument(skip_all))]
pub async fn fetch_module_manifest(
    client: &reqwest::Client,
    module_name: &str,
    version: &str,
) -> Result<ModuleManifest, FetchError> {
    let url = format!(
        "https://raw.githubusercontent.com/asimov-modules/asimov-{module_name}-module/{version}/.asimov/module.yaml",
    );

    let response = client.get(&url).send().await.inspect_err(|_err| {
        #[cfg(feature = "tracing")]
        tracing::debug!(err = ?_err);
    })?;

    if !response.status().is_success() {
        Err(HttpError::NotSuccess(response.status()))?;
    }

    let content = response.text().await.inspect_err(|_err| {
        #[cfg(feature = "tracing")]
        tracing::debug!(err = ?_err);
    })?;

    serde_yaml_ng::from_str(&content)
        .inspect_err(|_err| {
            #[cfg(feature = "tracing")]
            tracing::debug!(err = ?_err, ?content);
        })
        .map_err(|e| FetchError::Deserialize(e.into()))
}

#[cfg_attr(feature = "tracing", tracing::instrument(skip_all))]
pub async fn fetch_readme(
    client: &reqwest::Client,
    module_name: &str,
    version: &str,
) -> Option<String> {
    // prefer the README of the released version over the one on the default branch
    for revision in [version, "master", "main"] {
        let url = format!(
            "https://raw.githubusercontent.com/asimov-modules/asimov-{module_name}-module/{revision}/README.md",
        );

        #[cfg(feature = "tracing")]
        tracing::debug!("trying README URL {url}...");

        let response = match client.get(&url).send().await {
            Ok(response) => response,
            Err(_err) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(err = ?_err);
                continue;
            },
        };

        if !response.status().is_success() {
            continue;
        }

        match response.text().await {
            Ok(content) => return Some(content),
            Err(_err) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(err = ?_err);
            },
        }
    }

    None
}

#[cfg_attr(feature = "tracing", tracing::instrument(skip_all))]
pub async fn fetch_checksum(
    client: &reqwest::Client,
    asset_url: &str,
) -> Result<Option<String>, FetchChecksumError> {
    let checksum_url = format!("{asset_url}.sha256");

    let response = client.get(&checksum_url).send().await.inspect_err(|_err| {
        #[cfg(feature = "tracing")]
        tracing::debug!(err = ?_err);
    })?;

    if response.status() == 404 {
        return Ok(None);
    }

    if !response.status().is_success() {
        Err(HttpError::NotSuccess(response.status()))?;
    }

    Ok(Some(
        response
            .text()
            .await
            .inspect_err(|_err| {
                #[cfg(feature = "tracing")]
                tracing::debug!(err = ?_err);
            })?
            .trim()
            .to_string(),
    ))
}

pub async fn verify_checksum(
    binary_path: &Path,
    expected_checksum: &str,
) -> Result<(), VerifyChecksumError> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    let mut file = tokio::fs::File::open(binary_path).await?;

    const READ_BUFFER_SIZE: usize = 10 * 1024;
    let mut buf = vec![0u8; READ_BUFFER_SIZE];

    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break; // End of file
        }
        hasher.update(&buf[..n]);
    }

    let actual_checksum = format!("{:x}", hasher.finalize());

    // Extract just the hash part from expected (in case it has filename)
    let expected_hash = expected_checksum
        .split_whitespace()
        .next()
        .unwrap_or(expected_checksum);

    if actual_checksum != expected_hash {
        return Err(VerifyChecksumError::InvalidChecksum(
            actual_checksum,
            expected_checksum.into(),
        ));
    }

    Ok(())
}

#[cfg_attr(feature = "tracing", tracing::instrument(skip_all))]
pub async fn download_matching_asset(
    client: &reqwest::Client,
    module_name: &str,
    version: &str,
    platform: &super::platform::PlatformInfo,
    dst_dir: &Path,
) -> Result<(String, PathBuf), DownloadError> {
    let filenames = if let Some(libc) = &platform.libc {
        vec![
            format!(
                "asimov-{}-module-{}-{}-{}.tar.gz",
                module_name, platform.os, platform.arch, libc
            ),
            format!(
                "asimov-{}-module-{}-{}-{}.zip",
                module_name, platform.os, platform.arch, libc
            ),
            format!(
                "asimov-{}-module-{}-{}.tar.gz",
                module_name, platform.os, platform.arch
            ),
            format!(
                "asimov-{}-module-{}-{}.zip",
                module_name, platform.os, platform.arch
            ),
        ]
    } else {
        vec![
            format!(
                "asimov-{}-module-{}-{}.tar.gz",
                module_name, platform.os, platform.arch
            ),
            format!(
                "asimov-{}-module-{}-{}.zip",
                module_name, platform.os, platform.arch
            ),
        ]
    };

    for filename in filenames {
        let url = format!(
            "https://github.com/asimov-modules/asimov-{module_name}-module/releases/download/{version}/{filename}"
        );

        #[cfg(feature = "tracing")]
        tracing::debug!("trying asset URL {url}...");

        let mut response = client.get(&url).send().await.inspect_err(|_err| {
            #[cfg(feature = "tracing")]
            tracing::debug!(err = ?_err);
        })?;

        if response.status() == 404 {
            // try another asset pattern
            continue;
        }
        if !response.status().is_success() {
            Err(HttpError::NotSuccess(response.status()))?;
        }

        let asset_path = dst_dir.join(&filename);
        let mut dst = tokio::fs::File::create(&asset_path).await?;

        while let Some(chunk) = response.chunk().await.inspect_err(|_err| {
            #[cfg(feature = "tracing")]
            tracing::debug!(err = ?_err);
        })? {
            dst.write_all(&chunk).await?;
        }
        dst.flush().await?;

        return Ok((url, asset_path));
    }

    Err(DownloadError::NoMatch)
}

pub async fn extract_files(
    src_archive: impl AsRef<Path>,
    dst_dir: impl AsRef<Path>,
) -> Result<(), tokio::io::Error> {
    use std::io::{Error, Result};

    let src_archive = src_archive.as_ref().to_owned();
    let dst = dst_dir.as_ref().to_owned();

    let Some(src_name) = src_archive
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .map(String::from)
    else {
        return Err(Error::other("Unsupported format"));
    };

    tokio::task::spawn_blocking(move || -> Result<()> {
        let asset_file = std::fs::File::open(&src_archive)?;
        if src_name.ends_with(".tar.gz") {
            let gz = flate2::read::GzDecoder::new(asset_file);
            let mut archive = tar::Archive::new(gz);
            archive.unpack(&dst)?;
        } else if src_name.ends_with(".zip") {
            let mut archive = zip::ZipArchive::new(asset_file)?;
            archive.extract(&dst)?;
        } else {
            return Err(Error::other("Unsupported format"));
        }
        Ok(())
    })
    .await??;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncBufReadExt as _;

    async fn release_fixture(status: &str, body: &str) -> Result<String, FetchError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/releases/latest", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = tokio::io::BufReader::new(stream);
            let mut line = String::new();
            loop {
                line.clear();
                assert!(stream.read_line(&mut line).await.unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
            }
            stream
                .get_mut()
                .write_all(response.as_bytes())
                .await
                .unwrap();
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();
        let result = fetch_release_tag(&client, &url).await;
        server.await.unwrap();
        result
    }

    #[tokio::test]
    async fn release_api_uses_tags_instead_of_display_titles() {
        for body in [
            r#"{"name":"A friendly release title","tag_name":"v1.2.3"}"#,
            r#"{"name":null,"tag_name":"v1.2.3"}"#,
            r#"{"tag_name":"v1.2.3"}"#,
        ] {
            assert_eq!(release_fixture("200 OK", body).await.unwrap(), "v1.2.3");
        }
        assert!(matches!(
            release_fixture("200 OK", r#"{"name":"v1.2.3"}"#).await,
            Err(FetchError::Deserialize(_))
        ));
    }

    #[tokio::test]
    async fn release_api_preserves_http_errors() {
        assert!(matches!(
            release_fixture("403 Forbidden", r#"{"message":"rate limit exceeded"}"#).await,
            Err(FetchError::Http(HttpError::NotSuccess(status))) if status.as_u16() == 403
        ));
    }
}
