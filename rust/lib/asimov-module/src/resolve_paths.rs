// This is free and unencumbered software released into the public domain.

use super::{Resolver, error::FromDirError};
use std::fs;

#[test]
fn installed_and_legacy_manifests_share_precedence() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir(root.join("example")).unwrap();
    let paths = [
        "example/manifest.json",
        "example.json",
        "example.yaml",
        "example.yml",
    ];
    for (index, path) in paths.iter().enumerate() {
        // JSON is also valid YAML; each candidate has a distinct handler.
        fs::write(
            root.join(path),
            alloc::format!(r#"{{"name":"example","handles":{{"url_protocols":["p{index}"]}}}}"#),
        )
        .unwrap();
    }
    // Transaction data and unrelated directories must never be registered.
    fs::create_dir_all(root.join(".upgrade-example/old")).unwrap();
    fs::write(root.join(".upgrade-example/manifest.json"), "invalid").unwrap();
    fs::create_dir(root.join("unrelated")).unwrap();
    fs::write(root.join("README.md"), "invalid").unwrap();
    for (selected, path) in paths.iter().enumerate() {
        let resolver = Resolver::try_from_dir(root).unwrap();
        for index in 0..paths.len() {
            let result = resolver.resolve(&alloc::format!("p{index}:value")).unwrap();
            assert_eq!(result.len(), usize::from(index == selected));
        }
        fs::write(root.join(path), "{ invalid").unwrap();
        let error = Resolver::try_from_dir(root).unwrap_err();
        assert!(matches!(
            error,
            FromDirError::Parse { .. } | FromDirError::ParseJson { .. }
        ));
        fs::remove_file(root.join(path)).unwrap();
    }
    assert!(
        Resolver::try_from_dir(root)
            .unwrap()
            .resolve("p0:value")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn directory_and_manifest_io_errors_are_reported() {
    let root = tempfile::tempdir().unwrap();
    assert!(matches!(
        Resolver::try_from_dir(root.path().join("absent")),
        Err(FromDirError::ManifestDirIo { .. })
    ));
    fs::create_dir_all(root.path().join("example/manifest.json")).unwrap();
    assert!(matches!(
        Resolver::try_from_dir(root.path()),
        Err(FromDirError::ManifestIo { .. })
    ));
}

#[cfg(unix)]
#[test]
fn installed_manifest_symlinks_are_confined_to_the_module_directory() {
    let root = tempfile::tempdir().unwrap();
    let module = root.path().join("example");
    fs::create_dir(&module).unwrap();
    fs::write(root.path().join("other.json"), r#"{"name":"other"}"#).unwrap();
    std::os::unix::fs::symlink("../other.json", module.join("manifest.json")).unwrap();
    assert!(matches!(
        Resolver::try_from_dir(root.path()),
        Err(FromDirError::ManifestIo { .. })
    ));
}
