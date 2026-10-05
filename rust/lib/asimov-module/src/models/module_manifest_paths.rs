// This is free and unencumbered software released into the public domain.

use super::*;
use alloc::vec;

fn manifest() -> ModuleManifest {
    ModuleManifest {
        name: "example".into(),
        config: Some(Configuration {
            variables: vec![ConfigurationVariable {
                name: "api_key".into(),
                default_value: Some("fallback".into()),
                ..Default::default()
            }],
        }),
        ..Default::default()
    }
}

const INVALID: &[&str] = &[
    "",
    ".",
    "..",
    "../secret",
    "/tmp/secret",
    "a/b",
    "a\\b",
    "C:\\secret",
    "C:secret",
    "\\\\server\\share",
    "key:stream",
    "key\0",
    "key.",
    "key ",
    "NUL",
    "con.txt",
];

#[test]
fn rejects_components_before_root_resolution_or_value_lookup() {
    let mut manifest = manifest();
    // Validation also precedes the environment and default-value shortcuts.
    manifest.config.as_mut().unwrap().variables[0].environment = Some("PATH".into());
    for invalid in INVALID {
        for (name, key, profile, component) in [
            (*invalid, "api_key", "default", "module name"),
            ("example", *invalid, "default", "variable name"),
            ("example", "api_key", *invalid, "profile name"),
        ] {
            manifest.name = name.into();
            manifest.config.as_mut().unwrap().variables[0].name = key.into();
            let result = manifest.variable_with_root(key, Some(profile), || {
                panic!("resolved root for {invalid:?}")
            });
            assert!(
                matches!(result, Err(ReadVarError::InvalidComponent { component: found, .. }) if found == component)
            );
        }
        assert!(matches!(
            manifest.read_variables(Some(invalid)),
            Err(ReadVarError::InvalidComponent { .. })
        ));
    }
}

#[test]
fn safe_configuration_names_preserve_lookup_and_fallback() {
    let root = tempfile::tempdir().unwrap();
    let mut manifest = manifest();
    let read = |manifest: &ModuleManifest| {
        manifest.variable_with_root("api_key", Some("my.profile"), || root.path().into())
    };
    assert_eq!(read(&manifest).unwrap(), "fallback");
    let directory = root.path().join("configs/my.profile/example");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("api_key"), "configured").unwrap();
    assert_eq!(read(&manifest).unwrap(), "configured");
    std::fs::remove_file(directory.join("api_key")).unwrap();
    manifest.config.as_mut().unwrap().variables[0].default_value = None;
    assert!(matches!(
        read(&manifest),
        Err(ReadVarError::UnconfiguredVar(_))
    ));

    // Use an existing environment entry without mutating process-global state.
    if let Some((name, value)) = std::env::vars().next() {
        manifest.config.as_mut().unwrap().variables[0].environment = Some(name);
        assert_eq!(
            manifest
                .variable_with_root("api_key", None, || panic!(
                    "environment override accessed files"
                ))
                .unwrap(),
            value
        );
    }
}

#[cfg(feature = "serde")]
#[test]
fn manifest_names_are_checked_before_reading_and_extensions_are_appended() {
    for invalid in INVALID {
        let result = ModuleManifest::read_manifest_with_root(invalid, || {
            panic!("resolved root for {invalid:?}")
        });
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
    }
    let root = tempfile::tempdir().unwrap();
    let installed = root.path().join("modules/installed");
    std::fs::create_dir_all(&installed).unwrap();
    std::fs::write(installed.join("example.json"), r#"{"name":"example"}"#).unwrap();
    let read = |name| ModuleManifest::read_manifest_with_root(name, || root.path().into());
    assert_eq!(read("example").unwrap().name, "example");
    assert_eq!(
        read("example.other").unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    std::fs::write(
        installed.join("example.other.json"),
        r#"{"name":"example.other"}"#,
    )
    .unwrap();
    assert_eq!(read("example.other").unwrap().name, "example.other");
    std::fs::write(installed.join("legacy.yaml"), "name: legacy\n").unwrap();
    assert_eq!(read("legacy").unwrap().name, "legacy");
    std::fs::write(root.path().join("modules/old.yaml"), "name: old\n").unwrap();
    assert_eq!(read("old").unwrap().name, "old");
}

#[cfg(unix)]
#[test]
fn configuration_symlinks_cannot_escape_any_directory_boundary() {
    use std::os::unix::fs::symlink;
    for component in [
        "configs",
        "configs/default",
        "configs/default/example",
        "configs/default/example/api_key",
    ] {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = root.path().join(component);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        symlink(outside.path(), &link).unwrap();
        // Even with a default configured, escaping links must not look absent.
        let result = manifest().variable_with_root("api_key", None, || root.path().into());
        assert!(
            matches!(result, Err(ReadVarError::Io { .. })),
            "{component}: {result:?}"
        );
    }

    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("configs/default/example");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("actual"), "inside").unwrap();
    symlink("actual", directory.join("api_key")).unwrap();
    assert_eq!(
        manifest()
            .variable_with_root("api_key", None, || root.path().into())
            .unwrap(),
        "inside"
    );
    std::fs::remove_file(directory.join("api_key")).unwrap();
    symlink("../missing", directory.join("api_key")).unwrap();
    assert!(matches!(
        manifest().variable_with_root("api_key", None, || root.path().into()),
        Err(ReadVarError::Io { .. })
    ));
}

#[cfg(all(unix, feature = "serde"))]
#[test]
fn manifest_symlinks_cannot_escape_search_directories() {
    use std::os::unix::fs::symlink;
    for component in [
        "modules",
        "modules/installed",
        "modules/installed/example.json",
    ] {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("example.json"), r#"{"name":"outside"}"#).unwrap();
        let link = root.path().join(component);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        let target = if component.ends_with(".json") {
            outside.path().join("example.json")
        } else {
            outside.path().into()
        };
        symlink(target, link).unwrap();
        let error =
            ModuleManifest::read_manifest_with_root("example", || root.path().into()).unwrap_err();
        assert_ne!(error.kind(), std::io::ErrorKind::NotFound, "{component}");
    }
    let root = tempfile::tempdir().unwrap();
    let installed = root.path().join("modules/installed");
    std::fs::create_dir_all(&installed).unwrap();
    symlink("../missing.json", installed.join("example.json")).unwrap();
    std::fs::write(installed.join("example.yaml"), "name: fallback\n").unwrap();
    assert_ne!(
        ModuleManifest::read_manifest_with_root("example", || root.path().into())
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::NotFound
    );
}
