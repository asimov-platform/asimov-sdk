// This is free and unencumbered software released into the public domain.

#![cfg(feature = "fs")]

extern crate alloc;

use alloc::{format, string::String};
use asimov_directory::{UrlFile, UrlsFile, fs::StateDirectory};
use std::{
    io::{self, ErrorKind, Read},
    time::{Duration, UNIX_EPOCH},
};

fn urls(values: &[&str]) -> UrlsFile {
    UrlsFile::new(values).unwrap()
}

#[test]
fn storage_is_lazy_and_no_op_updates_preserve_the_file() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    let absent = urls(&["https://absent.example"]);
    assert!(resources.files()?.is_empty());
    assert_eq!(resources.read_urls("tracked")?, UrlsFile::default());
    assert_eq!(resources.remove_urls("tracked", &absent)?, 0);
    assert_eq!(resources.append_urls("tracked", &UrlsFile::default())?, 0);
    assert!(!state.join("resources").exists());

    let initial = urls(&[
        "https://b.example",
        "https://a.example",
        "https://b.example",
    ]);
    assert_eq!(resources.append_urls("tracked", &initial)?, 2);
    let path = state.join("resources/tracked.urls");
    let file = std::fs::File::options().write(true).open(&path)?;
    file.set_modified(UNIX_EPOCH + Duration::from_secs(1234567890))?;
    let original_permissions = file.metadata()?.permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    file.set_permissions(permissions)?;
    let before = std::fs::metadata(&path)?;
    assert_eq!(resources.append_urls("tracked", &initial)?, 0);
    assert_eq!(resources.remove_urls("tracked", &absent)?, 0);
    assert_eq!(resources.remove_urls("missing/nested", &absent)?, 0);
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

#[test]
fn appends_preserve_existing_bytes_and_removals_replace_atomically() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    resources.append_urls("tracked", &urls(&["https://a.example"]))?;
    let path = state.join("resources/tracked.urls");
    let original = "https://b.example\nhttps://a.example\nhttps://b.example\n";
    std::fs::write(&path, original)?;
    let mut opened = std::fs::File::open(&path)?;
    assert_eq!(
        resources.append_urls(
            "tracked",
            &urls(&[
                "https://a.example",
                "https://c.example",
                "https://c.example",
                "https://d.example"
            ])
        )?,
        2
    );
    let appended = format!("{original}https://c.example\nhttps://d.example\n");
    let mut observed = String::new();
    opened.read_to_string(&mut observed)?;
    assert_eq!(observed, appended, "append must retain the original file");

    let mut old = std::fs::File::open(&path)?;
    assert_eq!(
        resources.remove_urls("tracked", &urls(&["https://b.example"]))?,
        2
    );
    let mut observed = String::new();
    old.read_to_string(&mut observed)?;
    assert_eq!(
        observed, appended,
        "removal must replace, not truncate, the file"
    );
    assert_eq!(
        std::fs::read_to_string(&path)?,
        "https://a.example\nhttps://c.example\nhttps://d.example\n"
    );
    let remaining = resources.read_urls("tracked")?;
    assert_eq!(resources.remove_urls("tracked", &remaining)?, 3);
    assert_eq!(std::fs::read(&path)?, b"");
    assert_eq!(resources.files()?, ["tracked.urls"]);
    assert_eq!(std::fs::read_dir(state.join("resources"))?.count(), 1);
    Ok(())
}

#[test]
fn nested_collections_support_both_formats() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    let single = UrlFile::new("https://single.example").unwrap();
    assert!(resources.write_url("my/x/tweet", &single)?);
    assert!(!resources.write_url("my/x/tweet", &single)?);
    resources.append_urls(
        "my/x/tweets",
        &urls(&["https://first.example", "https://second.example"]),
    )?;
    std::fs::write(state.join("resources/ignored.txt"), "not a collection")?;
    assert_eq!(resources.files()?, ["my/x/tweet.url", "my/x/tweets.urls"]);
    assert_eq!(resources.read_url("my/x/tweet")?, single);
    assert_eq!(
        std::fs::read_to_string(state.join("resources/my/x/tweet.url"))?,
        "https://single.example\n"
    );
    assert_eq!(
        resources.read_urls("my/x/tweets")?.urls(),
        ["https://first.example", "https://second.example"]
    );
    assert!(
        resources
            .append_urls("my/x/tweet", &urls(&["https://other.example"]))
            .is_err()
    );
    assert!(resources.write_url("my/x/tweets", &single).is_err());
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

#[test]
fn invalid_names_never_create_storage() -> io::Result<()> {
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
            resources.write_url(name, &single).map(|_| ()),
            resources.read_urls(name).map(|_| ()),
            resources.append_urls(name, &batch).map(|_| ()),
            resources.remove_urls(name, &batch).map(|_| ()),
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

#[test]
fn malformed_collections_are_not_overwritten() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let resources = state.resources()?;
    let batch = urls(&["https://example.com"]);
    resources.append_urls("tracked", &batch)?;
    let path = state.join("resources/tracked.urls");
    for contents in [b"https://example.com".as_slice(), b"not a URL\n", b"\xff\n"] {
        std::fs::write(&path, contents)?;
        assert_eq!(
            resources.read_urls("tracked").unwrap_err().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(
            resources.append_urls("tracked", &batch).unwrap_err().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(
            resources.remove_urls("tracked", &batch).unwrap_err().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(std::fs::read(&path)?, contents);
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn links_cannot_redirect_access_outside_resources() -> io::Result<()> {
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
        assert!(resources.append_urls("tracked", &batch).is_err());
        assert!(resources.remove_urls("tracked", &batch).is_err());
        std::fs::remove_file(state.join("resources"))?;
    }
    std::fs::create_dir(state.join("resources"))?;
    symlink(&sibling, state.join("resources/nested"))?;
    assert!(resources.files().is_err());
    assert!(resources.read_urls("nested/tracked").is_err());
    assert!(resources.append_urls("nested/new", &batch).is_err());
    assert!(resources.remove_urls("nested/tracked", &batch).is_err());
    assert!(!sibling.join("new.urls").exists());
    std::fs::remove_file(state.join("resources/nested"))?;

    for target in [sibling.join("tracked.urls"), sibling.join("missing.urls")] {
        symlink(&target, state.join("resources/tracked.urls"))?;
        assert!(resources.files().is_err());
        assert!(resources.read_urls("tracked").is_err());
        assert!(resources.append_urls("tracked", &batch).is_err());
        assert!(resources.remove_urls("tracked", &batch).is_err());
        std::fs::remove_file(state.join("resources/tracked.urls"))?;
    }
    assert!(!sibling.join("missing.urls").exists());
    std::fs::hard_link(
        sibling.join("tracked.urls"),
        state.join("resources/tracked.urls"),
    )?;
    assert!(resources.files().is_err());
    assert!(resources.read_urls("tracked").is_err());
    assert!(resources.append_urls("tracked", &batch).is_err());
    assert!(resources.remove_urls("tracked", &batch).is_err());
    assert_eq!(
        std::fs::read_to_string(sibling.join("tracked.urls"))?,
        "https://untouched.example\n"
    );
    Ok(())
}
