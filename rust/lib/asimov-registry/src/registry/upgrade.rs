// This is free and unencumbered software released into the public domain.

use super::{BIN_DIR_NAME, MANIFEST_FILE_NAME, ModuleName, Registry};
use alloc::{collections::BTreeSet, format, vec::Vec};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

impl Registry {
    /// Recover an interrupted registration, upgrade, or uninstall for a module.
    ///
    /// Uncommitted directory and link moves are rolled back. Committed upgrades
    /// only need backup cleanup. Recovery errors retain the transaction for retry.
    pub async fn recover_upgrade(&self, name: &ModuleName) -> io::Result<()> {
        let registry = self.clone();
        let name = name.clone();
        tokio::task::spawn_blocking(move || {
            let _lock = registry.upgrade_lock()?;
            recover(&registry.transaction_dir(&name))
        })
        .await
        .map_err(io::Error::other)?
    }

    /// Replace an installed module with an assembled temporary directory.
    ///
    /// The enabled link is preserved. Publication failures restore the previous
    /// directory and executable links; failed rollback retains a recovery journal.
    /// Dropping the future does not cancel publication once the worker starts.
    /// After process interruption, call [`Self::recover_upgrade`] before use.
    /// This serializes registrations, upgrades, and uninstalls, but callers must
    /// exclude low-level mutations and enable/disable operations. Filesystem entries
    /// cannot be switched atomically: readers may see a brief publication gap.
    pub async fn replace_module(
        &self,
        name: &ModuleName,
        staged: tempfile::TempDir,
    ) -> io::Result<()> {
        let registry = self.clone();
        let name = name.clone();
        tokio::task::spawn_blocking(move || {
            let _lock = registry.upgrade_lock()?;
            registry.recover_publications()?;
            let transaction = std::path::absolute(registry.transaction_dir(&name))?;
            let result = registry
                .prepare_upgrade(&name, staged.path(), &transaction)
                .and_then(|moves| publish(&transaction, &moves, |_| Ok(())));
            if let Err(error) = result {
                return match recover(&transaction) {
                    Ok(()) => Err(error),
                    Err(rollback) => Err(io::Error::other(format!(
                        "upgrade failed: {error}; recovery at {} failed: {rollback}",
                        transaction.display()
                    ))),
                };
            }
            // Once committed, cleanup failure must not report a failed upgrade.
            // The next recovery retries cleanup without rolling back.
            let _ = recover(&transaction);
            Ok(())
        })
        .await
        .map_err(io::Error::other)?
    }

    pub(super) fn upgrade_lock(&self) -> io::Result<fs::File> {
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.install_dir.join(".upgrade.lock"))?;
        file.lock()?;
        Ok(file)
    }

    pub(super) fn transaction_dir(&self, name: &ModuleName) -> PathBuf {
        self.install_dir.join(format!(".upgrade-{name}"))
    }

    fn prepare_upgrade(
        &self,
        name: &ModuleName,
        staged: &Path,
        transaction: &Path,
    ) -> io::Result<Vec<(PathBuf, PathBuf)>> {
        let live = std::path::absolute(self.module_dir(name))?;
        let staged = fs::canonicalize(staged)?;
        for protected in [&live, &self.exec_dir, &self.enable_dir] {
            let protected = fs::canonicalize(protected)?;
            if staged.starts_with(&protected) || protected.starts_with(&staged) {
                return Err(io::Error::other(
                    "staging directory overlaps installed state",
                ));
            }
        }
        let old = validate_module(&live, name)?;
        let new = validate_module(&staged, name)?;
        let mut links = Vec::new();
        for program in old.union(&new) {
            let link = std::path::absolute(self.exec_dir.join(program))?;
            let target = match fs::symlink_metadata(&link) {
                Ok(metadata) if metadata.is_symlink() => {
                    let target = fs::read_link(&link)?;
                    if !old.contains(program)
                        || fs::canonicalize(self.exec_dir.join(&target))?
                            != fs::canonicalize(live.join(BIN_DIR_NAME).join(program))?
                    {
                        return Err(io::Error::other(format!("executable collision: {program}")));
                    }
                    Some(target)
                },
                Ok(_) => return Err(io::Error::other(format!("executable collision: {program}"))),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            links.push((program.clone(), link, target.is_some()));
        }
        fs::create_dir(transaction)?;
        fs::create_dir(transaction.join("old-links"))?;
        fs::create_dir(transaction.join("new-links"))?;
        fs::rename(staged, transaction.join("new"))?;
        let mut moves = Vec::new();
        for (program, link, existed) in links {
            if existed {
                moves.push((link.clone(), transaction.join("old-links").join(&program)));
            }
            if new.contains(&program) {
                let staged_link = transaction.join("new-links").join(&program);
                symlink(
                    &std::path::absolute(live.join(BIN_DIR_NAME).join(&program))?,
                    &staged_link,
                )?;
                moves.push((staged_link, link));
            }
        }
        // Move directories first; existing enabled links keep their stable target.
        moves.insert(0, (transaction.join("new"), live.clone()));
        moves.insert(0, (live, transaction.join("old")));
        write_journal(transaction, &moves)?;
        Ok(moves)
    }
}

pub(super) fn write_journal(transaction: &Path, moves: &[(PathBuf, PathBuf)]) -> io::Result<()> {
    let journal = fs::File::create(transaction.join("journal.tmp"))?;
    serde_json::to_writer(&journal, &moves).map_err(io::Error::other)?;
    journal.sync_all()?;
    fs::rename(
        transaction.join("journal.tmp"),
        transaction.join("journal.json"),
    )?;
    Ok(())
}

pub(super) fn validate_module(
    path: &Path,
    name: &ModuleName,
) -> io::Result<BTreeSet<alloc::string::String>> {
    if !fs::symlink_metadata(path)?.is_dir()
        || !fs::symlink_metadata(path.join(MANIFEST_FILE_NAME))?.is_file()
    {
        return Err(io::Error::other(
            "module directory and manifest must not be symlinks",
        ));
    }
    let manifest: asimov_module::InstalledModuleManifest =
        serde_json::from_slice(&fs::read(path.join(MANIFEST_FILE_NAME))?)
            .map_err(io::Error::other)?;
    if manifest.manifest.name != name.as_str() {
        return Err(io::Error::other("replacement module identity mismatch"));
    }
    let mut programs = BTreeSet::new();
    let bin = path.join(BIN_DIR_NAME);
    match fs::symlink_metadata(&bin) {
        Ok(metadata) if metadata.is_dir() => {},
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                && manifest.manifest.provides.programs.is_empty() =>
        {
            return Ok(programs);
        },
        Err(error) => return Err(error),
        Ok(_) => return Err(io::Error::other("module bin directory is not a directory")),
    }
    for program in manifest.manifest.provides.programs {
        asimov_core::validate_filename_component(&program).map_err(io::Error::other)?;
        if !fs::symlink_metadata(path.join(BIN_DIR_NAME).join(&program))?.is_file() {
            return Err(io::Error::other("module binary is not a regular file"));
        }
        if !programs.insert(program) {
            return Err(io::Error::other("duplicate module binary"));
        }
    }
    for entry in fs::read_dir(bin)? {
        let entry = entry?;
        if !entry.file_type()?.is_file()
            || !entry
                .file_name()
                .to_str()
                .is_some_and(|name| programs.contains(name))
        {
            return Err(io::Error::other("undeclared or non-regular module binary"));
        }
    }
    Ok(programs)
}

pub(super) fn publish(
    transaction: &Path,
    moves: &[(PathBuf, PathBuf)],
    mut before_move: impl FnMut(usize) -> io::Result<()>,
) -> io::Result<()> {
    for (index, (source, destination)) in moves.iter().enumerate() {
        before_move(index)?;
        fs::rename(source, destination)?;
    }
    // Existence is the commit point; no rollback is attempted after this point.
    fs::File::create(transaction.join("committed"))?;
    Ok(())
}

pub(super) fn recover(transaction: &Path) -> io::Result<()> {
    // Never interpret a partially deleted journal as a live transaction.
    let cleanup = transaction.with_extension("cleanup");
    if cleanup.try_exists()? {
        fs::remove_dir_all(&cleanup)?;
    }
    if !transaction.try_exists()? {
        return Ok(());
    }
    if !transaction.join("committed").try_exists()? {
        match fs::read(transaction.join("journal.json")) {
            Ok(bytes) => {
                let moves: Vec<(PathBuf, PathBuf)> =
                    serde_json::from_slice(&bytes).map_err(io::Error::other)?;
                for (source, destination) in moves.iter().rev() {
                    match fs::symlink_metadata(source) {
                        Ok(_) => continue,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {},
                        Err(error) => return Err(error),
                    }
                    match fs::symlink_metadata(destination) {
                        Ok(_) => fs::rename(destination, source)?,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {},
                        Err(error) => return Err(error),
                    }
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {},
            Err(error) => return Err(error),
        }
    }
    fs::rename(transaction, &cleanup)?;
    fs::remove_dir_all(cleanup)
}

#[cfg(unix)]
pub(super) fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}
#[cfg(windows)]
pub(super) fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(path: &Path, version: &str, programs: &[&str]) {
        fs::create_dir_all(path.join(BIN_DIR_NAME)).unwrap();
        let manifest = serde_json::json!({
            "name": "example", "version": version,
            "provides": {"programs": programs}
        });
        fs::write(
            path.join(MANIFEST_FILE_NAME),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        for program in programs {
            fs::write(path.join(BIN_DIR_NAME).join(program), version).unwrap();
        }
    }

    async fn fixture(root: &Path, enabled: bool) -> (Registry, ModuleName, tempfile::TempDir) {
        let registry = Registry::new(root, Default::default());
        registry.create_file_tree().await.unwrap();
        let name: ModuleName = "example".parse().unwrap();
        let old = root.join("old");
        module(&old, "1", &["shared", "removed"]);
        registry.add_module(&name, old).await.unwrap();
        if enabled {
            registry.enable_module(&name).await.unwrap();
        }
        let staged = tempfile::tempdir_in(root).unwrap();
        module(staged.path(), "2", &["shared", "added"]);
        (registry, name, staged)
    }

    async fn assert_old(registry: &Registry, name: &ModuleName) {
        assert_eq!(
            registry.module_version(name).await.unwrap().as_deref(),
            Some("1")
        );
        for program in ["shared", "removed"] {
            assert_eq!(
                fs::read_to_string(registry.exec_dir.join(program)).unwrap(),
                "1"
            );
        }
        assert!(fs::symlink_metadata(registry.exec_dir.join("added")).is_err());
        assert!(registry.is_module_enabled(name).await.unwrap());
        assert!(
            registry
                .enable_dir
                .join(name.as_str())
                .join(MANIFEST_FILE_NAME)
                .exists()
        );
    }

    #[tokio::test]
    async fn replacement_preserves_enabled_state_and_updates_program_set() {
        for enabled in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (registry, name, staged) = fixture(root.path(), enabled).await;
            registry.replace_module(&name, staged).await.unwrap();
            assert_eq!(
                registry.module_version(&name).await.unwrap().as_deref(),
                Some("2")
            );
            assert_eq!(registry.is_module_enabled(&name).await.unwrap(), enabled);
            for program in ["shared", "added"] {
                assert_eq!(
                    fs::read_to_string(registry.exec_dir.join(program)).unwrap(),
                    "2"
                );
            }
            assert!(fs::symlink_metadata(registry.exec_dir.join("removed")).is_err());
            assert!(!registry.transaction_dir(&name).exists());
        }
    }

    #[tokio::test]
    async fn missing_binary_and_link_collision_leave_old_installation_usable() {
        for missing in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let (registry, name, staged) = fixture(root.path(), true).await;
            if missing {
                fs::remove_file(staged.path().join("bin/shared")).unwrap();
            } else {
                fs::create_dir(registry.exec_dir.join("added")).unwrap();
            }
            assert!(registry.replace_module(&name, staged).await.is_err());
            if !missing {
                assert!(registry.exec_dir.join("added").is_dir());
                fs::remove_dir(registry.exec_dir.join("added")).unwrap();
            }
            assert_old(&registry, &name).await;
        }
    }

    #[tokio::test]
    async fn installed_directory_cannot_be_used_as_staging() {
        let root = tempfile::tempdir().unwrap();
        let (registry, name, _) = fixture(root.path(), true).await;
        assert!(
            registry
                .prepare_upgrade(
                    &name,
                    &registry.module_dir(&name),
                    &registry.transaction_dir(&name)
                )
                .is_err()
        );
        assert_old(&registry, &name).await;
    }

    #[tokio::test]
    async fn dropping_publication_future_keeps_staging_alive() {
        use core::{future::Future, task::Poll};
        let root = tempfile::tempdir().unwrap();
        let (registry, name, staged) = fixture(root.path(), true).await;
        let path = staged.path().to_path_buf();
        let lock = registry.upgrade_lock().unwrap();
        let mut future = alloc::boxed::Box::pin(registry.replace_module(&name, staged));
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
                if registry
                    .module_version(&name)
                    .await
                    .ok()
                    .flatten()
                    .as_deref()
                    == Some("2")
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
            fs::read_to_string(registry.exec_dir.join("shared")).unwrap(),
            "2"
        );
    }

    #[tokio::test]
    async fn every_interrupted_move_is_recoverable() {
        // Directory renames, added/removed links, and shared-link replacement.
        for completed in 0..=6 {
            let root = tempfile::tempdir().unwrap();
            let (registry, name, staged) = fixture(root.path(), true).await;
            let transaction = registry.transaction_dir(&name);
            let moves = registry
                .prepare_upgrade(&name, staged.path(), &transaction)
                .unwrap();
            assert_eq!(moves.len(), 6);
            for (source, destination) in moves.iter().take(completed) {
                fs::rename(source, destination).unwrap();
            }
            // Reopen as another process would, with no in-memory transaction state.
            let reopened = Registry::new(root.path(), Default::default());
            reopened.recover_upgrade(&name).await.unwrap();
            reopened.recover_upgrade(&name).await.unwrap();
            assert_old(&reopened, &name).await;
        }
    }

    #[tokio::test]
    async fn failed_directory_and_link_moves_roll_back() {
        for failed in 0..6 {
            let root = tempfile::tempdir().unwrap();
            let (registry, name, staged) = fixture(root.path(), true).await;
            let transaction = registry.transaction_dir(&name);
            let moves = registry
                .prepare_upgrade(&name, staged.path(), &transaction)
                .unwrap();
            let result = publish(&transaction, &moves, |index| {
                if index == failed {
                    Err(io::Error::other("injected rename failure"))
                } else {
                    Ok(())
                }
            });
            assert!(result.is_err());
            recover(&transaction).unwrap();
            assert_old(&registry, &name).await;
        }
    }

    #[tokio::test]
    async fn failed_recovery_keeps_backups_for_retry() {
        let root = tempfile::tempdir().unwrap();
        let (registry, name, staged) = fixture(root.path(), true).await;
        let transaction = registry.transaction_dir(&name);
        let moves = registry
            .prepare_upgrade(&name, staged.path(), &transaction)
            .unwrap();
        for (source, destination) in &moves {
            fs::rename(source, destination).unwrap();
        }
        let links = transaction.join("new-links");
        fs::remove_dir(&links).unwrap();
        fs::write(&links, "block rollback").unwrap();
        assert!(registry.recover_upgrade(&name).await.is_err());
        assert!(transaction.join("journal.json").exists());
        assert_eq!(
            fs::read_to_string(transaction.join("old/bin/shared")).unwrap(),
            "1"
        );
        fs::remove_file(&links).unwrap();
        fs::create_dir(&links).unwrap();
        registry.recover_upgrade(&name).await.unwrap();
        assert_old(&registry, &name).await;
    }

    #[tokio::test]
    async fn committed_upgrade_is_not_rolled_back_on_recovery() {
        let root = tempfile::tempdir().unwrap();
        let (registry, name, staged) = fixture(root.path(), true).await;
        let transaction = registry.transaction_dir(&name);
        let moves = registry
            .prepare_upgrade(&name, staged.path(), &transaction)
            .unwrap();
        publish(&transaction, &moves, |_| Ok(())).unwrap();
        // Simulate interruption after cleanup starts and removes its marker.
        let cleanup = transaction.with_extension("cleanup");
        fs::rename(&transaction, &cleanup).unwrap();
        fs::remove_file(cleanup.join("committed")).unwrap();
        registry.recover_upgrade(&name).await.unwrap();
        assert_eq!(
            registry.module_version(&name).await.unwrap().as_deref(),
            Some("2")
        );
        assert_eq!(
            fs::read_to_string(registry.exec_dir.join("shared")).unwrap(),
            "2"
        );
    }
}
