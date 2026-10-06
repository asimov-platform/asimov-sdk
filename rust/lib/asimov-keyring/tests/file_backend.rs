// This is free and unencumbered software released into the public domain.

#![cfg(all(feature = "std", unix))]

use asimov_keyring::{Keyring, KeyringError};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
};

// Re-execute the test binary so HOME and backend selection are isolated from
// the test runner, and persistence is checked across actual process lifetimes.
fn child(home: &Path, output: &Path, backend: Option<&str>) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "file_backend_child", "--nocapture"])
        .env("HOME", home)
        .env_remove("ASIMOV_KEYRING_BACKEND")
        .env("ASIMOV_KEYRING_TEST_OUTPUT", output)
        .env("ASIMOV_KEYRING_TEST_MODE", "ensure")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(backend) = backend {
        command.env("ASIMOV_KEYRING_BACKEND", backend);
    }
    command
}

#[test]
fn file_backend_child() {
    let Ok(mode) = std::env::var("ASIMOV_KEYRING_TEST_MODE") else {
        return;
    };
    if mode == "invalid" {
        assert!(matches!(
            Keyring::open(),
            Err(KeyringError::KeyringError(keyring_core::Error::Invalid(
                _,
                _
            )))
        ));
        return;
    }
    let mut start = [0];
    std::io::stdin().read_exact(&mut start).unwrap();
    let key = Keyring::my_public_key().unwrap();
    fs::write(
        std::env::var_os("ASIMOV_KEYRING_TEST_OUTPUT").unwrap(),
        key.to_string(),
    )
    .unwrap();
}

#[test]
fn concurrent_first_use_and_later_processes_share_one_identity() {
    check_concurrent_identity(Some("file"));
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires blocked Linux keyrings; run in Docker with --include-ignored"]
fn default_backend_works_with_blocked_kernel_keyrings() {
    use keyring_core::api::CredentialStoreApi;

    // Check the fixture without reading or writing any native secret.
    assert!(matches!(
        linux_keyutils_keyring_store::Store::new().unwrap().build(
            asimov_keyring::KEYRING_SERVICE,
            "default",
            None,
        ),
        Err(keyring_core::Error::NoStorageAccess(_))
    ));
    check_concurrent_identity(None);
}

fn check_concurrent_identity(backend: Option<&str>) {
    let home = tempfile::tempdir().unwrap();
    let mut children: Vec<_> = (0..8)
        .map(|index| {
            child(home.path(), &home.path().join(index.to_string()), backend)
                .spawn()
                .unwrap()
        })
        .collect();
    for child in &mut children {
        child.stdin.take().unwrap().write_all(b"!").unwrap();
    }
    let outputs: Vec<_> = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect();
    for output in outputs {
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let first = fs::read(home.path().join("0")).unwrap();
    for index in 1..8 {
        assert_eq!(
            fs::read(home.path().join(index.to_string())).unwrap(),
            first
        );
    }
    let mut later = child(home.path(), &home.path().join("later"), backend)
        .spawn()
        .unwrap();
    later.stdin.take().unwrap().write_all(b"!").unwrap();
    let output = later.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(home.path().join("later")).unwrap(), first);
    assert!(home.path().join(".asimov/.keyring").is_dir());
}

#[cfg(target_os = "linux")]
#[test]
fn automatic_selection_reuses_file_identity_without_configuration() {
    let home = tempfile::tempdir().unwrap();
    for (name, backend) in [("explicit", Some("file")), ("automatic", None)] {
        let mut process = child(home.path(), &home.path().join(name), backend)
            .spawn()
            .unwrap();
        process.stdin.take().unwrap().write_all(b"!").unwrap();
        let output = process.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        fs::read(home.path().join("explicit")).unwrap(),
        fs::read(home.path().join("automatic")).unwrap()
    );
}

#[test]
fn invalid_backend_names_fail_without_falling_back() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    for backend in [
        OsString::from(""),
        OsString::from("typo"),
        OsString::from_vec(vec![255]),
    ] {
        let home = tempfile::tempdir().unwrap();
        let output = child(home.path(), &home.path().join("unused"), None)
            .env("ASIMOV_KEYRING_BACKEND", backend)
            .env("ASIMOV_KEYRING_TEST_MODE", "invalid")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!home.path().join(".asimov/.keyring").exists());
    }
}
