// This is free and unencumbered software released into the public domain.

#![cfg(feature = "fs")]

extern crate alloc;

use alloc::{format, vec::Vec};
use asimov_directory::fs::StateDirectory;
use asimov_remote::RemotesRegistry;
use std::io::ErrorKind;

#[test]
fn tracked_remotes_round_trip_without_url_normalization() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let registry = RemotesRegistry::open(&state)?;
    assert!(registry.list()?.is_empty());
    for (name, url) in [
        ("origin", "https://EXAMPLE.com"),
        ("local", "http://localhost:8080/api?key=a%20b#fragment"),
        ("ipv6", "http://[::1]:8080/"),
    ] {
        registry.add(name, url)?;
        assert_eq!(state.remotes()?.read(name)?, format!("{url}\n"));
        assert_eq!(registry.get(name)?.url, url);
    }
    let remotes = registry.list()?;
    assert_eq!(
        remotes
            .iter()
            .map(|remote| remote.name.as_str())
            .collect::<Vec<_>>(),
        ["ipv6", "local", "origin"]
    );
    registry.rename("origin", "upstream")?;
    assert_eq!(registry.get("upstream")?.url, "https://EXAMPLE.com");
    registry.remove("upstream")?;
    assert_eq!(
        registry.get("upstream").unwrap_err().kind(),
        ErrorKind::NotFound
    );
    Ok(())
}

#[test]
fn unsupported_or_malformed_urls_do_not_create_files() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let registry = RemotesRegistry::open(&state)?;
    for url in [
        "",
        "example.com",
        "//example.com",
        "https:",
        "https://",
        "http:example.com",
        "http:///example.com",
        "https://example.com\\path",
        "file:///tmp/remote",
        "ftp://example.com",
        "iroh://example.com",
        "mailto:a@example.com",
        "https://example.com\n",
        "https://exam\tple.com",
        " https://example.com",
        "https://example.com/a b",
        "https://example.com\nhttps://other.example",
        "http://[invalid]/",
        "https://example.com:99999",
    ] {
        assert_eq!(
            registry.add("origin", url).unwrap_err().kind(),
            ErrorKind::InvalidInput,
            "{url:?}"
        );
    }
    assert!(!state.join("remotes").exists());
    Ok(())
}

#[test]
fn malformed_files_are_reported_and_can_be_removed() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let registry = RemotesRegistry::open(&state)?;
    registry.add("origin", "https://example.com")?;
    let path = state.join("remotes/origin");
    for contents in [
        b"".as_slice(),
        b"https://example.com",
        b"\n",
        b"iroh://example.com\n",
        b"https://example.com\n\n",
        b"https://example.com\nhttp://other.example\n",
        b"\xff\n",
    ] {
        std::fs::write(&path, contents)?;
        assert_eq!(
            registry.get("origin").unwrap_err().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(registry.list().unwrap_err().kind(), ErrorKind::InvalidData);
    }
    registry.remove("origin")?;
    assert!(registry.list()?.is_empty());
    std::fs::write(&path, b"https://example.com\r\n")?;
    assert_eq!(registry.get("origin")?.url, "https://example.com");
    Ok(())
}
