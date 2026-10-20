// This is free and unencumbered software released into the public domain.

#![cfg(feature = "fs")]

extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use asimov_directory::{
    UrlFile, UrlReader, UrlsFile,
    fs::{ResourcesDirectory, StateDirectory},
};
use std::{
    io::{self, ErrorKind, Read},
    time::{Duration, UNIX_EPOCH},
};

fn urls(values: &[&str]) -> UrlsFile {
    UrlsFile::new(values).unwrap()
}

async fn collect_urls(resources: &ResourcesDirectory, collection: &str) -> io::Result<UrlsFile> {
    let mut reader = resources.read_urls(collection)?;
    let mut urls = Vec::new();
    while let Some(url) = reader.next_url().await? {
        urls.push(url.to_string());
    }
    Ok(UrlsFile::new(urls).unwrap())
}

#[tokio::test]
async fn storage_is_lazy_and_no_op_updates_preserve_the_file() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    let absent = urls(&["https://absent.example"]);
    assert!(resources.files()?.is_empty());
    assert!(resources.read_urls("tracked")?.next_url().await?.is_none());
    assert_eq!(resources.remove_urls("tracked", &absent).await?, 0);
    assert_eq!(
        resources
            .append_urls("tracked", &UrlsFile::default())
            .await?,
        0
    );
    assert!(!state.join("resources").exists());

    let initial = urls(&[
        "https://b.example",
        "https://a.example",
        "https://b.example",
    ]);
    assert_eq!(resources.append_urls("tracked", &initial).await?, 2);
    let path = state.join("resources/tracked.urls");
    let file = std::fs::File::options().write(true).open(&path)?;
    file.set_modified(UNIX_EPOCH + Duration::from_secs(1234567890))?;
    let original_permissions = file.metadata()?.permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    file.set_permissions(permissions)?;
    let before = std::fs::metadata(&path)?;
    assert_eq!(resources.append_urls("tracked", &initial).await?, 0);
    assert_eq!(resources.remove_urls("tracked", &absent).await?, 0);
    assert_eq!(resources.remove_urls("missing/nested", &absent).await?, 0);
    assert!(!state.join("resources/missing").exists());
    let after = std::fs::metadata(&path)?;
    assert_eq!(after.modified()?, before.modified()?);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            (after.ino(), after.ctime(), after.ctime_nsec()),
            (before.ino(), before.ctime(), before.ctime_nsec())
        );
    }
    assert_eq!(resources.files()?, ["tracked.urls"]);
    assert_eq!(
        std::fs::read_to_string(&path)?,
        "https://b.example\nhttps://a.example\n"
    );
    file.set_permissions(original_permissions)?;
    Ok(())
}

#[tokio::test]
async fn appends_preserve_existing_bytes_and_removals_replace_atomically() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    resources
        .append_urls("tracked", &urls(&["https://a.example"]))
        .await?;
    let path = state.join("resources/tracked.urls");
    let original = "https://b.example\nhttps://a.example\nhttps://b.example\n";
    std::fs::write(&path, original)?;
    let mut opened = std::fs::File::open(&path)?;
    assert_eq!(
        resources
            .append_urls(
                "tracked",
                &urls(&[
                    "https://a.example",
                    "https://d.example",
                    "https://d.example",
                    "https://c.example"
                ])
            )
            .await?,
        2
    );
    let appended = format!("{original}https://d.example\nhttps://c.example\n");
    let mut observed = String::new();
    opened.read_to_string(&mut observed)?;
    assert_eq!(observed, appended, "append must retain the original file");

    let mut old = std::fs::File::open(&path)?;
    assert_eq!(
        resources
            .remove_urls("tracked", &urls(&["https://a.example"]))
            .await?,
        1
    );
    let mut observed = String::new();
    old.read_to_string(&mut observed)?;
    assert_eq!(
        observed, appended,
        "removal must replace, not truncate, the file"
    );
    assert_eq!(
        std::fs::read_to_string(&path)?,
        "https://b.example\nhttps://b.example\nhttps://d.example\nhttps://c.example\n"
    );
    assert_eq!(
        resources
            .remove_urls("tracked", &urls(&["https://b.example"]))
            .await?,
        2
    );
    assert_eq!(
        std::fs::read_to_string(&path)?,
        "https://d.example\nhttps://c.example\n"
    );
    let remaining = collect_urls(&resources, "tracked").await?;
    assert_eq!(resources.remove_urls("tracked", &remaining).await?, 2);
    assert_eq!(std::fs::read(&path)?, b"");
    assert_eq!(resources.files()?, ["tracked.urls"]);
    assert_eq!(std::fs::read_dir(state.join("resources"))?.count(), 1);
    Ok(())
}

#[tokio::test]
async fn streaming_updates_preserve_order_across_many_buffers() -> io::Result<()> {
    use std::io::Write;

    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    resources
        .append_urls("tracked", &urls(&["urn:placeholder"]))
        .await?;
    let path = state.join("resources/tracked.urls");
    let mut fixture = std::io::BufWriter::new(std::fs::File::create(&path)?);
    for index in 0..20_000 {
        writeln!(fixture, "urn:item:{index}")?;
    }
    fixture.flush()?;
    drop(fixture);

    assert_eq!(
        resources
            .append_urls(
                "tracked",
                &urls(&[
                    "urn:item:19999",
                    "urn:new:one",
                    "urn:new:one",
                    "urn:new:two"
                ])
            )
            .await?,
        2
    );
    assert_eq!(
        resources
            .remove_urls(
                "tracked",
                &urls(&[
                    "urn:item:0",
                    "urn:item:10000",
                    "urn:item:19999",
                    "urn:absent"
                ])
            )
            .await?,
        3
    );
    let mut reader = resources.read_urls("tracked")?;
    for index in (1..19_999).filter(|index| *index != 10_000) {
        assert_eq!(
            reader.next_url().await?,
            Some(format!("urn:item:{index}").as_str())
        );
    }
    assert_eq!(reader.next_url().await?, Some("urn:new:one"));
    assert_eq!(reader.next_url().await?, Some("urn:new:two"));
    assert_eq!(reader.next_url().await?, None);
    assert_eq!(std::fs::read_dir(state.join("resources"))?.count(), 1);
    Ok(())
}

#[tokio::test]
async fn nested_collections_support_both_formats() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    let single = UrlFile::new("https://single.example").unwrap();
    assert!(resources.write_url("my/x/tweet", &single).await?);
    assert!(!resources.write_url("my/x/tweet", &single).await?);
    resources
        .append_urls(
            "my/x/tweets",
            &urls(&["https://first.example", "https://second.example"]),
        )
        .await?;
    std::fs::write(state.join("resources/ignored.txt"), "not a collection")?;
    assert_eq!(resources.files()?, ["my/x/tweet.url", "my/x/tweets.urls"]);
    let mut reader = resources.read_url("my/x/tweet")?;
    assert_eq!(reader.next_url().await?, Some(single.url()));
    assert_eq!(reader.next_url().await?, None);
    assert_eq!(
        std::fs::read_to_string(state.join("resources/my/x/tweet.url"))?,
        "https://single.example\n"
    );
    assert_eq!(
        collect_urls(&resources, "my/x/tweets").await?.urls(),
        ["https://first.example", "https://second.example"]
    );
    assert!(
        resources
            .append_urls("my/x/tweet", &urls(&["https://other.example"]))
            .await
            .is_err()
    );
    assert!(resources.write_url("my/x/tweets", &single).await.is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in ["resources", "resources/my", "resources/my/x"] {
            assert_eq!(
                std::fs::metadata(state.join(path))?.permissions().mode() & 0o777,
                0o700
            );
        }
        for path in ["resources/my/x/tweet.url", "resources/my/x/tweets.urls"] {
            assert_eq!(
                std::fs::metadata(state.join(path))?.permissions().mode() & 0o777,
                0o600
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn single_url_reads_ignore_trailing_content_in_either_format() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    assert_eq!(
        resources.read_url("missing").unwrap_err().kind(),
        ErrorKind::NotFound
    );
    assert!(!state.join("resources").exists());
    let first = UrlFile::new("urn:first").unwrap();
    resources.write_url("single", &first).await?;
    std::fs::write(state.join("resources/single.url"), b"urn:first\n\xff\n")?;
    let mut reader = resources.read_url("single")?;
    assert_eq!(reader.next_url().await?, Some("urn:first"));
    assert_eq!(reader.next_url().await?, None);

    resources
        .append_urls("multiple", &urls(&["urn:first", "urn:second"]))
        .await?;
    let mut reader = UrlReader::from(resources.read_urls("multiple")?);
    assert_eq!(reader.next_url().await?, Some("urn:first"));
    assert_eq!(reader.next_url().await?, None);
    assert_eq!(reader.into_inner().next_url().await?, Some("urn:second"));
    Ok(())
}

#[tokio::test]
async fn single_url_writes_replace_atomically_and_reject_extra_records() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    let first = UrlFile::new("urn:first").unwrap();
    let second = UrlFile::new("urn:second").unwrap();
    resources.write_url("single", &first).await?;
    let path = state.join("resources/single.url");
    let mut old = std::fs::File::open(&path)?;
    assert!(resources.write_url("single", &second).await?);
    let mut observed = String::new();
    old.read_to_string(&mut observed)?;
    assert_eq!(observed, "urn:first\n");
    assert_eq!(std::fs::read_to_string(&path)?, "urn:second\n");
    assert!(!resources.write_url("single", &second).await?);

    for contents in [
        b"".as_slice(),
        b"urn:first",
        b"urn:first\nurn:second\n",
        b"urn:first\n\xff\n",
    ] {
        std::fs::write(&path, contents)?;
        for replacement in [&first, &second] {
            assert_eq!(
                resources
                    .write_url("single", replacement)
                    .await
                    .unwrap_err()
                    .kind(),
                ErrorKind::InvalidData
            );
            assert_eq!(std::fs::read(&path)?, contents);
        }
        assert_eq!(std::fs::read_dir(state.join("resources"))?.count(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn invalid_names_never_create_storage() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    let batch = urls(&["https://example.com"]);
    let single = UrlFile::new("https://example.com").unwrap();
    for name in [
        "",
        ".",
        "..",
        "../escape",
        "a/../escape",
        "/absolute",
        "a//b",
        "a/",
        "./a",
        "a\\b",
        "C:/drive",
        "a/NUL",
        "a/b:stream",
        "a\0",
        "a\n",
    ] {
        for result in [
            resources.read_url(name).map(|_| ()),
            resources.write_url(name, &single).await.map(|_| ()),
            resources.read_urls(name).map(|_| ()),
            resources.append_urls(name, &batch).await.map(|_| ()),
            resources.remove_urls(name, &batch).await.map(|_| ()),
        ] {
            assert_eq!(
                result.unwrap_err().kind(),
                ErrorKind::InvalidInput,
                "{name:?}"
            );
        }
    }
    assert!(!state.join("resources").exists());
    Ok(())
}

#[tokio::test]
async fn malformed_collections_are_not_overwritten() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    let batch = urls(&["https://example.com"]);
    resources.append_urls("tracked", &batch).await?;
    let path = state.join("resources/tracked.urls");
    for contents in [b"https://example.com".as_slice(), b"not a URL\n", b"\xff\n"] {
        std::fs::write(&path, contents)?;
        assert_eq!(
            resources
                .read_urls("tracked")?
                .next_url()
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(
            resources
                .append_urls("tracked", &batch)
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(
            resources
                .remove_urls("tracked", &batch)
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(std::fs::read(&path)?, contents);
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn links_cannot_redirect_access_outside_resources() -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let outside = tempfile::tempdir()?;
    let sibling = state.join("private");
    std::fs::create_dir(&sibling)?;
    let resources = state.resources()?;
    let batch = urls(&["https://untouched.example"]);
    for target in [outside.path(), sibling.as_std_path()] {
        std::fs::write(target.join("tracked.urls"), "https://untouched.example\n")?;
        symlink(target, state.join("resources"))?;
        assert!(resources.files().is_err());
        assert!(resources.read_urls("tracked").is_err());
        assert!(resources.append_urls("tracked", &batch).await.is_err());
        assert!(resources.remove_urls("tracked", &batch).await.is_err());
        std::fs::remove_file(state.join("resources"))?;
    }
    std::fs::create_dir(state.join("resources"))?;
    symlink(&sibling, state.join("resources/nested"))?;
    assert!(resources.files().is_err());
    assert!(resources.read_urls("nested/tracked").is_err());
    assert!(resources.append_urls("nested/new", &batch).await.is_err());
    assert!(
        resources
            .remove_urls("nested/tracked", &batch)
            .await
            .is_err()
    );
    assert!(!sibling.join("new.urls").exists());
    std::fs::remove_file(state.join("resources/nested"))?;

    for target in [sibling.join("tracked.urls"), sibling.join("missing.urls")] {
        symlink(&target, state.join("resources/tracked.urls"))?;
        assert!(resources.files().is_err());
        assert!(resources.read_urls("tracked").is_err());
        assert!(resources.append_urls("tracked", &batch).await.is_err());
        assert!(resources.remove_urls("tracked", &batch).await.is_err());
        std::fs::remove_file(state.join("resources/tracked.urls"))?;
    }
    assert!(!sibling.join("missing.urls").exists());
    std::fs::hard_link(
        sibling.join("tracked.urls"),
        state.join("resources/tracked.urls"),
    )?;
    assert!(resources.files().is_err());
    assert!(resources.read_urls("tracked").is_err());
    assert!(resources.append_urls("tracked", &batch).await.is_err());
    assert!(resources.remove_urls("tracked", &batch).await.is_err());
    assert_eq!(
        std::fs::read_to_string(sibling.join("tracked.urls"))?,
        "https://untouched.example\n"
    );
    Ok(())
}
