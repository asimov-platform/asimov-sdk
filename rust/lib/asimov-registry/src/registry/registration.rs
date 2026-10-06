// This is free and unencumbered software released into the public domain.

use super::upgrade::{publish, recover, symlink, validate_module, write_journal};
use super::{AddModuleError, BIN_DIR_NAME, MANIFEST_FILE_NAME, ModuleName, Registry};
use alloc::{format, vec::Vec};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

impl Registry {
    /// Register a module, rejecting occupied executable names before mutation.
    ///
    /// The manifest must match `name` and declare exactly the regular files in
    /// `bin`. Publication errors roll back the source directory and links.
    /// Failed recovery retains its journal and is reported along with the error.
    /// Keep `dir` and its parents stable until publication finishes, including
    /// after cancellation. Prefer [`Self::add_module_owned`] for temporary data.
    /// Interrupted operations are recovered by the next registration, upgrade,
    /// or uninstall. Low-level mutations must not run concurrently.
    pub async fn add_module(
        &self,
        name: &ModuleName,
        dir: impl AsRef<Path>,
    ) -> Result<(), AddModuleError> {
        self.register(name, dir.as_ref().to_path_buf(), None).await
    }

    /// Register an owned staging directory that survives caller cancellation.
    ///
    /// Uses the validation, collision, and rollback rules of [`Self::add_module`].
    pub async fn add_module_owned(
        &self,
        name: &ModuleName,
        dir: tempfile::TempDir,
    ) -> Result<(), AddModuleError> {
        self.register(name, dir.path().to_path_buf(), Some(dir))
            .await
    }

    async fn register(
        &self,
        name: &ModuleName,
        staged: PathBuf,
        guard: Option<tempfile::TempDir>,
    ) -> Result<(), AddModuleError> {
        let registry = self.clone();
        let name = name.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            let _lock = registry.upgrade_lock()?;
            registry.recover_publications()?;
            let live = registry.module_dir(&name);
            if exists(&live)? {
                return Err(AddModuleError::AlreadyInstalled);
            }
            for extension in ["json", "yaml", "yml"] {
                if exists(&registry.install_dir.join(format!("{name}.{extension}")))? {
                    return Err(AddModuleError::AlreadyInstalled);
                }
            }
            let transaction = std::path::absolute(registry.transaction_dir(&name))?;
            let result = registry
                .prepare_registration(&name, &staged, &transaction)
                .and_then(|moves| publish(&transaction, &moves, |_| Ok(())));
            finish(&transaction, result).map_err(Into::into)
        })
        .await
        .map_err(io::Error::other)?
    }

    fn prepare_registration(
        &self,
        name: &ModuleName,
        staged: &Path,
        transaction: &Path,
    ) -> io::Result<Vec<(PathBuf, PathBuf)>> {
        let programs = validate_module(staged, name)?;
        let staged = fs::canonicalize(staged)?;
        let live = std::path::absolute(self.module_dir(name))?;
        // Never move a staging ancestor containing registry state.
        for protected in [&self.install_dir, &self.exec_dir, &self.enable_dir] {
            let protected = fs::canonicalize(protected)?;
            if protected.starts_with(&staged) || staged.starts_with(&protected) {
                return Err(io::Error::other(
                    "staging directory overlaps registry state",
                ));
            }
        }
        for program in &programs {
            if exists(&self.exec_dir.join(program))? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("executable collision: {program}"),
                ));
            }
        }
        fs::create_dir(transaction)?;
        fs::create_dir(transaction.join("links"))?;
        let mut moves = Vec::new();
        moves.push((staged, live.clone()));
        for program in programs {
            let link = transaction.join("links").join(&program);
            symlink(&live.join(BIN_DIR_NAME).join(&program), &link)?;
            moves.push((link, std::path::absolute(self.exec_dir.join(program))?));
        }
        write_journal(transaction, &moves)?;
        Ok(moves)
    }

    /// Uninstall a module and only the executable/enabled links it still owns.
    ///
    /// Foreign links and regular files are preserved, including when the
    /// manifest still lists their names. Failures roll back all completed moves;
    /// interrupted publication is recoverable by the next mutation. This shares
    /// the registration/upgrade lock. Low-level mutations must be excluded.
    pub async fn uninstall_module(&self, name: &ModuleName) -> io::Result<()> {
        self.recover_upgrade(name).await?;
        self.migrate_legacy_manifest(name).await;
        let registry = self.clone();
        let name = name.clone();
        tokio::task::spawn_blocking(move || {
            let _lock = registry.upgrade_lock()?;
            registry.recover_publications()?;
            let transaction = std::path::absolute(registry.transaction_dir(&name))?;
            let result = registry
                .prepare_uninstall(&name, &transaction)
                .and_then(|moves| publish(&transaction, &moves, |_| Ok(())));
            finish(&transaction, result)
        })
        .await
        .map_err(io::Error::other)?
    }

    fn prepare_uninstall(
        &self,
        name: &ModuleName,
        transaction: &Path,
    ) -> io::Result<Vec<(PathBuf, PathBuf)>> {
        let live = std::path::absolute(self.module_dir(name))?;
        if !fs::symlink_metadata(&live)?.is_dir() {
            return Err(io::Error::other("installed module is not a directory"));
        }
        match fs::symlink_metadata(live.join(BIN_DIR_NAME)) {
            Ok(metadata) if metadata.is_dir() => {},
            Ok(_) => return Err(io::Error::other("module bin directory is not a directory")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {},
            Err(error) => return Err(error),
        }
        let manifest: asimov_module::InstalledModuleManifest =
            serde_json::from_slice(&fs::read(live.join(MANIFEST_FILE_NAME))?)
                .map_err(io::Error::other)?;
        let mut owned = Vec::new();
        for program in manifest.manifest.provides.programs {
            asimov_core::validate_filename_component(&program).map_err(io::Error::other)?;
            let link = std::path::absolute(self.exec_dir.join(&program))?;
            if owns_link(&link, &live.join(BIN_DIR_NAME).join(&program))? {
                owned.push((link, transaction.join("links").join(program)));
            }
        }
        owned.sort();
        owned.dedup();
        let enabled = std::path::absolute(self.enable_dir.join(name.as_str()))?;
        if owns_link(&enabled, &live)? {
            owned.push((enabled, transaction.join("enabled")));
        }
        fs::create_dir(transaction)?;
        fs::create_dir(transaction.join("links"))?;
        owned.push((live, transaction.join("old")));
        write_journal(transaction, &owned)?;
        Ok(owned)
    }

    pub(super) fn recover_publications(&self) -> io::Result<()> {
        let mut names = alloc::collections::BTreeSet::new();
        for entry in fs::read_dir(&self.install_dir)? {
            let entry = entry?;
            let filename = entry.file_name();
            let Some(name) = filename
                .to_str()
                .and_then(|name| name.strip_prefix(".upgrade-"))
            else {
                continue;
            };
            let name = name.strip_suffix(".cleanup").unwrap_or(name);
            if let Ok(name) = ModuleName::try_from(name) {
                names.insert(name);
            }
        }
        for name in names {
            recover(&self.transaction_dir(&name))?;
        }
        Ok(())
    }
}

fn finish(transaction: &Path, result: io::Result<()>) -> io::Result<()> {
    if let Err(error) = result {
        return match recover(transaction) {
            Ok(()) => Err(error),
            Err(rollback) => Err(io::Error::other(format!(
                "publication failed: {error}; recovery at {} failed: {rollback}",
                transaction.display()
            ))),
        };
    }
    let _ = recover(transaction); // Committed cleanup is retried on next mutation.
    Ok(())
}

fn exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn owns_link(link: &Path, expected: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(link) {
        Ok(metadata) if metadata.is_symlink() => {},
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    }
    // Compare destinations rather than contents; this also recognizes dangling
    // owned links. Do not follow a foreign file symlink into the module.
    let target = link.parent().unwrap().join(fs::read_link(link)?);
    if target.file_name() != expected.file_name() {
        return Ok(false);
    }
    let parents = || -> io::Result<bool> {
        Ok(fs::canonicalize(target.parent().unwrap())?
            == fs::canonicalize(expected.parent().unwrap())?)
    };
    match parents() {
        Ok(owned) => Ok(owned),
        // If the parent cannot be resolved, ownership cannot be established.
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "registration_tests.rs"]
mod tests;
