// This is free and unencumbered software released into the public domain.

use super::*;

fn staged(root: &Path, name: &str, programs: &[&str]) -> tempfile::TempDir {
    let directory = tempfile::tempdir_in(root).unwrap();
    fs::create_dir(directory.path().join(BIN_DIR_NAME)).unwrap();
    fs::write(
        directory.path().join(MANIFEST_FILE_NAME),
        serde_json::to_vec(&serde_json::json!({"name": name, "provides": {"programs": programs}}))
            .unwrap(),
    )
    .unwrap();
    for program in programs {
        fs::write(directory.path().join(BIN_DIR_NAME).join(program), name).unwrap();
    }
    directory
}

async fn registry(root: &Path) -> Registry {
    let registry = Registry::new(root, Default::default());
    registry.create_file_tree().await.unwrap();
    registry
}

#[tokio::test]
async fn colliding_modules_never_replace_the_first_owner() {
    let root = tempfile::tempdir().unwrap();
    let registry = registry(root.path()).await;
    let first: ModuleName = "first".parse().unwrap();
    registry
        .add_module_owned(&first, staged(root.path(), "first", &["shared"]))
        .await
        .unwrap();
    let second: ModuleName = "second".parse().unwrap();
    let source = staged(root.path(), "second", &["shared", "unique"]);
    assert!(registry.add_module(&second, source.path()).await.is_err());
    assert_eq!(
        fs::read_to_string(registry.exec_dir.join("shared")).unwrap(),
        "first"
    );
    assert!(!exists(&registry.exec_dir.join("unique")).unwrap());
    assert!(!registry.module_dir(&second).exists());
    assert!(source.path().join("bin/shared").exists());
}

#[tokio::test]
async fn regular_files_directories_and_dangling_links_are_collisions() {
    for kind in ["file", "directory", "symlink"] {
        let root = tempfile::tempdir().unwrap();
        let registry = registry(root.path()).await;
        let link = registry.exec_dir.join("program");
        match kind {
            "file" => fs::write(&link, "keep").unwrap(),
            "directory" => fs::create_dir(&link).unwrap(),
            _ => symlink(Path::new("missing"), &link).unwrap(),
        }
        let name = "example".parse().unwrap();
        assert!(
            registry
                .add_module_owned(&name, staged(root.path(), "example", &["program"]))
                .await
                .is_err()
        );
        assert!(exists(&link).unwrap());
        assert!(!registry.module_dir(&name).exists());
        if kind == "file" {
            assert_eq!(fs::read_to_string(link).unwrap(), "keep");
        }
    }
}

#[tokio::test]
async fn malformed_executable_entries_are_rejected_before_publication() {
    for kind in [
        "missing",
        "undeclared",
        "directory",
        "symlink",
        "bin-symlink",
        "traversal",
        "identity",
    ] {
        let root = tempfile::tempdir().unwrap();
        let registry = registry(root.path()).await;
        let source = staged(root.path(), "example", &["program"]);
        let binary = source.path().join("bin/program");
        match kind {
            "missing" => fs::remove_file(&binary).unwrap(),
            "undeclared" => fs::write(source.path().join("bin/extra"), "extra").unwrap(),
            "directory" => {
                fs::remove_file(&binary).unwrap();
                fs::create_dir(&binary).unwrap();
            },
            "symlink" => {
                fs::remove_file(&binary).unwrap();
                symlink(Path::new("../manifest.json"), &binary).unwrap();
            },
            "bin-symlink" => {
                fs::rename(source.path().join("bin"), source.path().join("actual")).unwrap();
                #[cfg(unix)]
                std::os::unix::fs::symlink("actual", source.path().join("bin")).unwrap();
                #[cfg(windows)]
                std::os::windows::fs::symlink_dir("actual", source.path().join("bin")).unwrap();
            },
            _ => {
                let manifest = if kind == "identity" {
                    serde_json::json!({"name": "other"})
                } else {
                    serde_json::json!({"name": "example", "provides": {"programs": ["../outside"]}})
                };
                fs::write(
                    source.path().join(MANIFEST_FILE_NAME),
                    serde_json::to_vec(&manifest).unwrap(),
                )
                .unwrap();
            },
        }
        let name = "example".parse().unwrap();
        assert!(
            registry.add_module(&name, source.path()).await.is_err(),
            "{kind}"
        );
        assert!(!registry.module_dir(&name).exists());
        assert!(!exists(&registry.exec_dir.join("program")).unwrap());
        assert!(source.path().exists());
    }
}

#[tokio::test]
async fn partial_link_failure_restores_source_and_removes_published_links() {
    let root = tempfile::tempdir().unwrap();
    let registry = registry(root.path()).await;
    let name = "example".parse().unwrap();
    let source = staged(root.path(), "example", &["first", "second"]);
    let transaction = registry.transaction_dir(&name);
    let moves = registry
        .prepare_registration(&name, source.path(), &transaction)
        .unwrap();
    // Force a real rename failure after the module and first link are published.
    fs::create_dir(registry.exec_dir.join("second")).unwrap();
    assert!(finish(&transaction, publish(&transaction, &moves, |_| Ok(()))).is_err());
    assert!(!registry.module_dir(&name).exists());
    assert!(!exists(&registry.exec_dir.join("first")).unwrap());
    assert!(registry.exec_dir.join("second").is_dir());
    assert_eq!(
        fs::read_to_string(source.path().join("bin/first")).unwrap(),
        "example"
    );
    fs::remove_dir(registry.exec_dir.join("second")).unwrap();
    registry.add_module(&name, source.path()).await.unwrap();
}

#[tokio::test]
async fn interrupted_registration_recovers_before_another_module_claims_the_name() {
    for completed in 0..=3 {
        let root = tempfile::tempdir().unwrap();
        let registry = registry(root.path()).await;
        let name = "example".parse().unwrap();
        let source = staged(root.path(), "example", &["first", "second"]);
        let transaction = registry.transaction_dir(&name);
        let moves = registry
            .prepare_registration(&name, source.path(), &transaction)
            .unwrap();
        for (source, destination) in moves.iter().take(completed) {
            fs::rename(source, destination).unwrap();
        }
        let other = "other".parse().unwrap();
        registry
            .add_module_owned(&other, staged(root.path(), "other", &["first"]))
            .await
            .unwrap();
        assert!(!registry.module_dir(&name).exists());
        assert_eq!(
            fs::read_to_string(source.path().join("bin/first")).unwrap(),
            "example"
        );
        assert_eq!(
            fs::read_to_string(registry.exec_dir.join("first")).unwrap(),
            "other"
        );
        assert!(!exists(&registry.exec_dir.join("second")).unwrap());
    }
}

#[tokio::test]
async fn uninstall_preserves_reassigned_and_regular_executable_entries() {
    for kind in [
        "foreign",
        "dangling-foreign",
        "regular",
        "owned-relative",
        "owned-dangling",
    ] {
        let root = tempfile::tempdir().unwrap();
        let registry = registry(root.path()).await;
        let name = "example".parse().unwrap();
        registry
            .add_module_owned(&name, staged(root.path(), "example", &["program"]))
            .await
            .unwrap();
        registry.enable_module(&name).await.unwrap();
        let link = registry.exec_dir.join("program");
        fs::remove_file(&link).unwrap();
        match kind {
            "foreign" => {
                let other = "other".parse().unwrap();
                registry
                    .add_module_owned(&other, staged(root.path(), "other", &["other"]))
                    .await
                    .unwrap();
                symlink(&registry.module_dir(&other).join("bin/other"), &link).unwrap();
            },
            "dangling-foreign" => symlink(Path::new("missing"), &link).unwrap(),
            "regular" => fs::write(&link, "keep").unwrap(),
            _ => {
                symlink(Path::new("../modules/installed/example/bin/program"), &link).unwrap();
                if kind == "owned-dangling" {
                    fs::remove_file(registry.module_dir(&name).join("bin/program")).unwrap();
                }
            },
        }
        registry.uninstall_module(&name).await.unwrap();
        assert!(!registry.module_dir(&name).exists());
        assert!(!exists(&registry.enable_dir.join("example")).unwrap());
        assert_eq!(exists(&link).unwrap(), !kind.starts_with("owned"));
        if kind == "foreign" {
            assert_eq!(fs::read_to_string(link).unwrap(), "other");
        } else if kind == "regular" {
            assert_eq!(fs::read_to_string(link).unwrap(), "keep");
        }
    }
}

#[tokio::test]
async fn uninstall_failure_and_interruption_restore_enabled_module_and_links() {
    for completed in 0..=4 {
        let root = tempfile::tempdir().unwrap();
        let registry = registry(root.path()).await;
        let name = "example".parse().unwrap();
        registry
            .add_module_owned(&name, staged(root.path(), "example", &["first", "second"]))
            .await
            .unwrap();
        registry.enable_module(&name).await.unwrap();
        let transaction = registry.transaction_dir(&name);
        let moves = registry.prepare_uninstall(&name, &transaction).unwrap();
        assert_eq!(moves.len(), 4);
        for (source, destination) in moves.iter().take(completed) {
            fs::rename(source, destination).unwrap();
        }
        finish(
            &transaction,
            Err(io::Error::other("interrupted or failed uninstall")),
        )
        .unwrap_err();
        assert!(registry.module_dir(&name).exists());
        assert!(registry.enable_dir.join("example/manifest.json").exists());
        for program in ["first", "second"] {
            assert_eq!(
                fs::read_to_string(registry.exec_dir.join(program)).unwrap(),
                "example"
            );
        }
    }
}

#[tokio::test]
async fn concurrent_registrations_have_one_owner() {
    let root = tempfile::tempdir().unwrap();
    let registry = registry(root.path()).await;
    let first = "first".parse().unwrap();
    let second = "second".parse().unwrap();
    let (a, b) = tokio::join!(
        registry.add_module_owned(&first, staged(root.path(), "first", &["shared"])),
        registry.add_module_owned(&second, staged(root.path(), "second", &["shared"]))
    );
    assert_ne!(a.is_ok(), b.is_ok());
    assert_eq!(
        fs::read_to_string(registry.exec_dir.join("shared")).unwrap(),
        if a.is_ok() { "first" } else { "second" }
    );
}

#[cfg(unix)]
#[tokio::test]
async fn ownership_resolves_symlinked_parents_before_dot_dot() {
    let root = tempfile::tempdir().unwrap();
    let registry = registry(root.path()).await;
    let name = "example".parse().unwrap();
    registry
        .add_module_owned(&name, staged(root.path(), "example", &["program"]))
        .await
        .unwrap();
    let foreign = root.path().join("foreign");
    fs::create_dir_all(foreign.join("nested")).unwrap();
    fs::write(foreign.join("program"), "foreign").unwrap();
    let bin = registry.module_dir(&name).join("bin");
    std::os::unix::fs::symlink(foreign.join("nested"), bin.join("alias")).unwrap();
    let link = registry.exec_dir.join("program");
    fs::remove_file(&link).unwrap();
    symlink(&bin.join("alias/../program"), &link).unwrap();
    assert_eq!(fs::read_to_string(&link).unwrap(), "foreign");
    registry.uninstall_module(&name).await.unwrap();
    assert!(exists(&link).unwrap());
    assert_eq!(
        fs::read_to_string(foreign.join("program")).unwrap(),
        "foreign"
    );
}

#[tokio::test]
async fn cancelled_owned_registration_keeps_staging_alive() {
    use core::{future::Future, task::Poll};
    let root = tempfile::tempdir().unwrap();
    let registry = registry(root.path()).await;
    let name = "example".parse().unwrap();
    let source = staged(root.path(), "example", &["program"]);
    let path = source.path().to_path_buf();
    let lock = registry.upgrade_lock().unwrap();
    let mut future = alloc::boxed::Box::pin(registry.add_module_owned(&name, source));
    core::future::poll_fn(|context| {
        assert!(future.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(future);
    assert!(path.join(MANIFEST_FILE_NAME).exists());
    drop(lock);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if registry.exec_dir.join("program").exists()
                && !registry.transaction_dir(&name).exists()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        fs::read_to_string(registry.exec_dir.join("program")).unwrap(),
        "example"
    );
}
