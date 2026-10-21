// This is free and unencumbered software released into the public domain.

#![cfg(feature = "fs")]

extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use asimov_directory::{
    UriFile, UriReader, UrisFile,
    fs::{ResourcesDirectory, StateDirectory},
};
use std::{
    io::{self, ErrorKind, Read},
    time::{Duration, UNIX_EPOCH},
};

fn uris(values: &[&str]) -> UrisFile {
    UrisFile::new(values).unwrap()
}

async fn collect_uris(resources: &ResourcesDirectory, collection: &str) -> io::Result<UrisFile> {
    let mut reader = resources.read_uris(collection)?;
    let mut uris = Vec::new();
    while let Some(uri) = reader.next_uri().await? {
        uris.push(uri.to_string());
    }
    Ok(UrisFile::new(uris).unwrap())
}

#[tokio::test]
async fn storage_is_lazy_and_no_op_updates_preserve_the_file() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    let absent = uris(&["https://absent.example"]);
    assert!(resources.files()?.is_empty());
    assert!(resources.read_uris("tracked")?.next_uri().await?.is_none());
    assert_eq!(resources.remove_uris("tracked", &absent).await?, 0);
    assert_eq!(
        resources
            .append_uris("tracked", &UrisFile::default())
            .await?,
        0
    );
    assert!(!state.join("resources").exists());

    let initial = uris(&[
        "https://b.example",
        "https://a.example",
        "https://b.example",
    ]);
    assert_eq!(resources.append_uris("tracked", &initial).await?, 2);
    let path = state.join("resources/tracked.uris");
    let file = std::fs::File::options().write(true).open(&path)?;
    file.set_modified(UNIX_EPOCH + Duration::from_secs(1234567890))?;
    let original_permissions = file.metadata()?.permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    file.set_permissions(permissions)?;
    let before = std::fs::metadata(&path)?;
    assert_eq!(resources.append_uris("tracked", &initial).await?, 0);
    assert_eq!(resources.remove_uris("tracked", &absent).await?, 0);
    assert_eq!(resources.remove_uris("missing/nested", &absent).await?, 0);
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
    assert_eq!(resources.files()?, ["tracked.uris"]);
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
        .append_uris("tracked", &uris(&["https://a.example"]))
        .await?;
    let path = state.join("resources/tracked.uris");
    let original = "https://b.example\nhttps://a.example\nhttps://b.example\n";
    std::fs::write(&path, original)?;
    let mut opened = std::fs::File::open(&path)?;
    assert_eq!(
        resources
            .append_uris(
                "tracked",
                &uris(&[
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
            .remove_uris("tracked", &uris(&["https://a.example"]))
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
            .remove_uris("tracked", &uris(&["https://b.example"]))
            .await?,
        2
    );
    assert_eq!(
        std::fs::read_to_string(&path)?,
        "https://d.example\nhttps://c.example\n"
    );
    let remaining = collect_uris(&resources, "tracked").await?;
    assert_eq!(resources.remove_uris("tracked", &remaining).await?, 2);
    assert_eq!(std::fs::read(&path)?, b"");
    assert_eq!(resources.files()?, ["tracked.uris"]);
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
        .append_uris("tracked", &uris(&["urn:placeholder"]))
        .await?;
    let path = state.join("resources/tracked.uris");
    let mut fixture = std::io::BufWriter::new(std::fs::File::create(&path)?);
    for index in 0..20_000 {
        writeln!(fixture, "urn:item:{index}")?;
    }
    fixture.flush()?;
    drop(fixture);

    assert_eq!(
        resources
            .append_uris(
                "tracked",
                &uris(&[
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
            .remove_uris(
                "tracked",
                &uris(&[
                    "urn:item:0",
                    "urn:item:10000",
                    "urn:item:19999",
                    "urn:absent"
                ])
            )
            .await?,
        3
    );
    let mut reader = resources.read_uris("tracked")?;
    for index in (1..19_999).filter(|index| *index != 10_000) {
        assert_eq!(
            reader.next_uri().await?,
            Some(format!("urn:item:{index}").as_str())
        );
    }
    assert_eq!(reader.next_uri().await?, Some("urn:new:one"));
    assert_eq!(reader.next_uri().await?, Some("urn:new:two"));
    assert_eq!(reader.next_uri().await?, None);
    assert_eq!(std::fs::read_dir(state.join("resources"))?.count(), 1);
    Ok(())
}

#[tokio::test]
async fn nested_collections_support_both_formats() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    let single = UriFile::new("https://例え.テスト/🦀#片").unwrap();
    assert!(resources.write_uri("my/x/tweet", &single).await?);
    assert!(!resources.write_uri("my/x/tweet", &single).await?);
    resources
        .append_uris(
            "my/x/tweets",
            &uris(&["https://first.example", single.uri()]),
        )
        .await?;
    std::fs::write(state.join("resources/ignored.txt"), "not a collection")?;
    assert_eq!(resources.files()?, ["my/x/tweet.uri", "my/x/tweets.uris"]);
    let mut reader = resources.read_uri("my/x/tweet")?;
    assert_eq!(reader.next_uri().await?, Some(single.uri()));
    assert_eq!(reader.next_uri().await?, None);
    assert_eq!(
        std::fs::read_to_string(state.join("resources/my/x/tweet.uri"))?,
        "https://例え.テスト/🦀#片\n"
    );
    assert_eq!(
        collect_uris(&resources, "my/x/tweets").await?.uris(),
        ["https://first.example", single.uri()]
    );
    assert!(
        resources
            .append_uris("my/x/tweet", &uris(&["https://other.example"]))
            .await
            .is_err()
    );
    assert!(resources.write_uri("my/x/tweets", &single).await.is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in ["resources", "resources/my", "resources/my/x"] {
            assert_eq!(
                std::fs::metadata(state.join(path))?.permissions().mode() & 0o777,
                0o700
            );
        }
        for path in ["resources/my/x/tweet.uri", "resources/my/x/tweets.uris"] {
            assert_eq!(
                std::fs::metadata(state.join(path))?.permissions().mode() & 0o777,
                0o600
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn single_uri_reads_ignore_trailing_content_in_either_format() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    assert_eq!(
        resources.read_uri("missing").unwrap_err().kind(),
        ErrorKind::NotFound
    );
    assert!(!state.join("resources").exists());
    let first = UriFile::new("urn:first").unwrap();
    resources.write_uri("single", &first).await?;
    std::fs::write(state.join("resources/single.uri"), b"urn:first\n\xff\n")?;
    let mut reader = resources.read_uri("single")?;
    assert_eq!(reader.next_uri().await?, Some("urn:first"));
    assert_eq!(reader.next_uri().await?, None);

    resources
        .append_uris("multiple", &uris(&["urn:first", "urn:second"]))
        .await?;
    let mut reader = UriReader::from(resources.read_uris("multiple")?);
    assert_eq!(reader.next_uri().await?, Some("urn:first"));
    assert_eq!(reader.next_uri().await?, None);
    assert_eq!(reader.into_inner().next_uri().await?, Some("urn:second"));
    Ok(())
}

#[tokio::test]
async fn single_uri_writes_replace_atomically_and_reject_extra_records() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    let first = UriFile::new("urn:first").unwrap();
    let second = UriFile::new("urn:second").unwrap();
    resources.write_uri("single", &first).await?;
    let path = state.join("resources/single.uri");
    let mut old = std::fs::File::open(&path)?;
    assert!(resources.write_uri("single", &second).await?);
    let mut observed = String::new();
    old.read_to_string(&mut observed)?;
    assert_eq!(observed, "urn:first\n");
    assert_eq!(std::fs::read_to_string(&path)?, "urn:second\n");
    assert!(!resources.write_uri("single", &second).await?);

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
                    .write_uri("single", replacement)
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
    let batch = uris(&["https://example.com"]);
    let single = UriFile::new("https://example.com").unwrap();
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
            resources.read_uri(name).map(|_| ()),
            resources.write_uri(name, &single).await.map(|_| ()),
            resources.read_uris(name).map(|_| ()),
            resources.append_uris(name, &batch).await.map(|_| ()),
            resources.remove_uris(name, &batch).await.map(|_| ()),
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
    let batch = uris(&["https://example.com"]);
    resources.append_uris("tracked", &batch).await?;
    let path = state.join("resources/tracked.uris");
    for contents in [
        b"https://example.com".as_slice(),
        b"not an IRI\n",
        b"\xff\n",
        b"https://example.com\r\n",
        b"# comment\nhttps://example.com\n",
    ] {
        std::fs::write(&path, contents)?;
        assert_eq!(
            resources
                .read_uris("tracked")?
                .next_uri()
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(
            resources
                .append_uris("tracked", &batch)
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(
            resources
                .remove_uris("tracked", &batch)
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
    let batch = uris(&["https://untouched.example"]);
    for target in [outside.path(), sibling.as_std_path()] {
        std::fs::write(target.join("tracked.uris"), "https://untouched.example\n")?;
        symlink(target, state.join("resources"))?;
        assert!(resources.files().is_err());
        assert!(resources.read_uris("tracked").is_err());
        assert!(resources.append_uris("tracked", &batch).await.is_err());
        assert!(resources.remove_uris("tracked", &batch).await.is_err());
        std::fs::remove_file(state.join("resources"))?;
    }
    std::fs::create_dir(state.join("resources"))?;
    symlink(&sibling, state.join("resources/nested"))?;
    assert!(resources.files().is_err());
    assert!(resources.read_uris("nested/tracked").is_err());
    assert!(resources.append_uris("nested/new", &batch).await.is_err());
    assert!(
        resources
            .remove_uris("nested/tracked", &batch)
            .await
            .is_err()
    );
    assert!(!sibling.join("new.uris").exists());
    std::fs::remove_file(state.join("resources/nested"))?;

    for target in [sibling.join("tracked.uris"), sibling.join("missing.uris")] {
        symlink(&target, state.join("resources/tracked.uris"))?;
        assert!(resources.files().is_err());
        assert!(resources.read_uris("tracked").is_err());
        assert!(resources.append_uris("tracked", &batch).await.is_err());
        assert!(resources.remove_uris("tracked", &batch).await.is_err());
        std::fs::remove_file(state.join("resources/tracked.uris"))?;
    }
    assert!(!sibling.join("missing.uris").exists());
    std::fs::hard_link(
        sibling.join("tracked.uris"),
        state.join("resources/tracked.uris"),
    )?;
    assert!(resources.files().is_err());
    assert!(resources.read_uris("tracked").is_err());
    assert!(resources.append_uris("tracked", &batch).await.is_err());
    assert!(resources.remove_uris("tracked", &batch).await.is_err());
    assert_eq!(
        std::fs::read_to_string(sibling.join("tracked.uris"))?,
        "https://untouched.example\n"
    );
    Ok(())
}
