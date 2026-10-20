// This is free and unencumbered software released into the public domain.

#![cfg(feature = "fs")]

extern crate alloc;

use alloc::vec::Vec;
use asimov_directory::fs::StateDirectory;
use std::io::ErrorKind;

#[test]
fn remote_files_are_complete_and_never_overwritten() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let remotes = state.remotes()?;
    assert!(remotes.names()?.is_empty());
    assert!(!state.join("remotes").exists());
    remotes.create("origin", "https://example.com")?;
    assert_eq!(remotes.read("origin")?, "https://example.com\n");
    assert_eq!(
        remotes
            .create("origin", "http://other.example")
            .unwrap_err()
            .kind(),
        ErrorKind::AlreadyExists
    );
    remotes.create("backup", "http://localhost:8080/api")?;
    assert_eq!(remotes.names()?, ["backup", "origin"]);
    assert_eq!(
        remotes.rename("origin", "backup").unwrap_err().kind(),
        ErrorKind::AlreadyExists
    );
    assert_eq!(remotes.read("backup")?, "http://localhost:8080/api\n");
    remotes.rename("origin", "origin")?;
    remotes.rename("origin", "upstream")?;
    assert_eq!(
        remotes.read("origin").unwrap_err().kind(),
        ErrorKind::NotFound
    );
    assert_eq!(remotes.read("upstream")?, "https://example.com\n");
    remotes.remove("upstream")?;
    assert_eq!(
        remotes.remove("upstream").unwrap_err().kind(),
        ErrorKind::NotFound
    );
    assert_eq!(
        remotes.rename("absent", "absent").unwrap_err().kind(),
        ErrorKind::NotFound
    );
    Ok(())
}

#[test]
fn invalid_names_are_rejected_before_access() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let remotes = state.remotes()?;
    for name in [
        "",
        ".",
        "..",
        "../escape",
        "/absolute",
        "a/b",
        "a\\b",
        "C:drive",
        "NUL",
        "a\n",
    ] {
        for result in [
            remotes.create(name, "https://example.com"),
            remotes.read(name).map(|_| ()),
            remotes.rename(name, "valid"),
            remotes.rename("valid", name),
            remotes.remove(name),
        ] {
            assert_eq!(
                result.unwrap_err().kind(),
                ErrorKind::InvalidInput,
                "{name:?}"
            );
        }
    }
    for value in ["", "a\nb", "a\rb", "a\n"] {
        assert_eq!(
            remotes.create("valid", value).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
    }
    assert!(!state.join("remotes").exists());
    Ok(())
}

#[test]
fn concurrent_additions_publish_exactly_one_complete_file() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let remotes = state.remotes()?;
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let tasks: Vec<_> = ["https://one.example", "https://two.example"]
            .into_iter()
            .map(|url| {
                let remotes = &remotes;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    remotes.create("origin", url)
                })
            })
            .collect();
        let results: Vec<_> = tasks.into_iter().map(|task| task.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results.into_iter().find_map(Result::err).unwrap().kind(),
            ErrorKind::AlreadyExists
        );
    });
    assert!(matches!(
        remotes.read("origin")?.as_str(),
        "https://one.example\n" | "https://two.example\n"
    ));
    assert_eq!(remotes.names()?, ["origin"]);
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_redirect_remote_access() -> std::io::Result<()> {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir()?;
    let state = StateDirectory::open(temp.path())?;
    let outside = tempfile::tempdir()?;
    let outside_state = StateDirectory::open(outside.path())?;
    let target = outside_state.join("origin");
    std::fs::write(&target, "https://untouched.example\n")?;
    symlink(outside.path(), state.join("remotes"))?;
    let remotes = state.remotes()?;
    assert!(remotes.names().is_err());
    assert!(remotes.create("new", "https://example.com").is_err());
    assert!(remotes.read("origin").is_err());
    assert!(remotes.rename("origin", "new").is_err());
    assert!(remotes.remove("origin").is_err());
    std::fs::remove_file(state.join("remotes"))?;
    std::fs::create_dir(state.join("remotes"))?;
    symlink(&target, state.join("remotes/origin"))?;
    assert!(remotes.names().is_err());
    assert!(remotes.read("origin").is_err());
    assert!(remotes.rename("origin", "new").is_err());
    assert!(remotes.remove("origin").is_err());
    assert_eq!(
        remotes
            .create("origin", "https://example.com")
            .unwrap_err()
            .kind(),
        ErrorKind::AlreadyExists
    );
    assert_eq!(
        std::fs::read_to_string(target)?,
        "https://untouched.example\n"
    );
    assert!(!outside_state.join("new").exists());
    Ok(())
}
