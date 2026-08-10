// This is free and unencumbered software released into the public domain.

use alloc::{collections::BTreeSet, format, string::String, vec::Vec};
use asimov_module::InstalledModuleManifest;
pub use asimov_module::ModuleName;
use std::path::{Path, PathBuf};
use tokio::io;

pub mod error;
use error::*;

pub const MANIFEST_FILE_NAME: &str = "manifest.json";
pub const README_FILE_PATH: &str = "doc/README.md";
pub const BIN_DIR_NAME: &str = "bin";

#[derive(Clone, Debug, Default, bon::Builder)]
pub struct Options {}

#[derive(Clone, Debug)]
pub struct Registry {
    install_dir: PathBuf,
    enable_dir: PathBuf,
    exec_dir: PathBuf,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new(asimov_env::paths::asimov_root(), Options::default())
    }
}

impl Registry {
    pub fn new(asimov_dir: impl Into<PathBuf>, _options: Options) -> Self {
        let dir = asimov_dir.into();
        Self {
            install_dir: dir.join("modules").join("installed"),
            enable_dir: dir.join("modules").join("enabled"),
            exec_dir: dir.join("libexec"),
        }
    }

    pub fn with_dirs<S1, S2, S3>(
        install_dir: S1,
        enable_dir: S2,
        exec_dir: S3,
        _options: Options,
    ) -> Self
    where
        S1: Into<PathBuf>,
        S2: Into<PathBuf>,
        S3: Into<PathBuf>,
    {
        Self {
            install_dir: install_dir.into(),
            enable_dir: enable_dir.into(),
            exec_dir: exec_dir.into(),
        }
    }

    pub async fn create_file_tree(&self) -> Result<(), CreateFileTreeError> {
        tokio::fs::create_dir_all(&self.install_dir)
            .await
            .map_err(|e| CreateFileTreeError::InstallDir(self.install_dir.clone(), e))?;

        tokio::fs::create_dir_all(&self.enable_dir)
            .await
            .map_err(|e| CreateFileTreeError::EnableDir(self.enable_dir.clone(), e))?;

        tokio::fs::create_dir_all(&self.exec_dir)
            .await
            .map_err(|e| CreateFileTreeError::ExecDir(self.exec_dir.clone(), e))?;

        Ok(())
    }

    pub fn install_dir(&self) -> &Path {
        &self.install_dir
    }

    pub fn module_dir(&self, module_name: &ModuleName) -> PathBuf {
        self.install_dir.join(module_name.as_str())
    }

    pub async fn add_module(
        &self,
        module_name: &ModuleName,
        dir: impl AsRef<Path>,
    ) -> Result<(), AddModuleError> {
        if self.is_module_installed(module_name).await.unwrap_or(false) {
            return Err(AddModuleError::AlreadyInstalled);
        }

        let module_dir = self.module_dir(module_name);

        tokio::fs::rename(dir.as_ref(), &module_dir)
            .await
            .map_err(|e| AddModuleError::Install(dir.as_ref().into(), module_dir.clone(), e))?;

        let bin_dir = module_dir.join(BIN_DIR_NAME);

        let mut entries = match tokio::fs::read_dir(&bin_dir).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(AddModuleError::ReadBinDir(bin_dir, err)),
        };

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| AddModuleError::ReadBinDir(bin_dir.clone(), e))?
        {
            let program_name = entry.file_name();
            let Some(program_name) = program_name.to_str() else {
                continue;
            };

            self.add_binary(program_name, &entry.path())
                .await
                .map_err(|e| AddModuleError::AddBinary(program_name.into(), e))?;
        }

        Ok(())
    }

    pub async fn read_manifest(
        &self,
        module_name: &ModuleName,
    ) -> Result<InstalledModuleManifest, ManifestError> {
        self.migrate_legacy_manifest(module_name).await;

        let path = self
            .find_manifest_file(module_name)
            .await?
            .ok_or(ManifestError::NotInstalled)?;
        read_manifest(path).await.map_err(Into::into)
    }

    pub async fn read_readme(
        &self,
        module_name: &ModuleName,
    ) -> Result<Option<String>, ReadReadmeError> {
        if !self.is_module_installed(module_name).await? {
            return Err(ReadReadmeError::NotInstalled);
        }

        let readme_path = self.module_dir(module_name).join(README_FILE_PATH);

        match tokio::fs::read_to_string(&readme_path).await {
            Ok(content) => Ok(Some(content)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(ReadReadmeError::Read(readme_path, err)),
        }
    }

    pub async fn module_version(
        &self,
        module_name: &ModuleName,
    ) -> Result<Option<String>, ModuleVersionError> {
        self.read_manifest(module_name)
            .await
            .map(|manifest| manifest.version)
            .map_err(Into::into)
    }

    pub async fn remove_module(&self, module_name: &ModuleName) -> Result<(), RemoveModuleError> {
        self.migrate_legacy_manifest(module_name).await;

        if self.find_manifest_file(module_name).await?.is_none() {
            return Err(RemoveModuleError::NotInstalled);
        }

        let module_dir = self.module_dir(module_name);

        tokio::fs::remove_dir_all(&module_dir)
            .await
            .map_err(|e| RemoveModuleError::RemoveModuleDir(module_dir, e))
    }

    async fn add_binary(&self, program_name: &str, binary_path: &Path) -> io::Result<()> {
        let target_path = match self
            .exec_dir
            .parent()
            .and_then(|parent| binary_path.strip_prefix(parent).ok())
        {
            Some(suffix) => PathBuf::from("..").join(suffix),
            None => binary_path.into(),
        };

        let link_path = self.exec_dir.join(program_name);
        let _ = self.remove_binary(program_name).await;

        create_symlink(&target_path, &link_path, false).await
    }

    pub async fn remove_binary(&self, name: impl AsRef<str>) -> Result<(), RemoveBinaryError> {
        let name = name.as_ref();

        if !is_valid_program_name(name) {
            return Err(RemoveBinaryError::InvalidName(name.into()));
        }

        let binary_path = self.exec_dir.join(name);

        tokio::fs::remove_file(&binary_path).await.or_else(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(RemoveBinaryError::Io(e))
            }
        })
    }

    pub async fn installed_modules(
        &self,
    ) -> Result<Vec<InstalledModuleManifest>, InstalledModulesError> {
        let installed_dir = &self.install_dir;

        let mut module_names = BTreeSet::new();

        let mut read_dir = tokio::fs::read_dir(&installed_dir)
            .await
            .map_err(|e| InstalledModulesError::DirIo(installed_dir.clone(), e))?;

        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|e| InstalledModulesError::DirIo(installed_dir.clone(), e))?
        {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };

            if tokio::fs::metadata(entry.path())
                .await
                .map(|md| md.is_dir())
                .unwrap_or(false)
            {
                let Ok(module_name) = ModuleName::try_from(name) else {
                    continue;
                };

                module_names.insert(module_name);
            } else if let Some(name) = manifest_file_module_name(Path::new(&name)) {
                let Ok(module_name) = ModuleName::try_from(name) else {
                    continue;
                };

                self.migrate_legacy_manifest(&module_name).await;

                module_names.insert(module_name);
            }
        }

        let mut modules = Vec::new();

        for module_name in module_names {
            let manifest_path = self.module_dir(&module_name).join(MANIFEST_FILE_NAME);

            if !tokio::fs::try_exists(&manifest_path)
                .await
                .map_err(|e| InstalledModulesError::DirIo(manifest_path.clone(), e))?
            {
                continue;
            }

            let manifest = read_manifest(&manifest_path)
                .await
                .map_err(|e| InstalledModulesError::ReadManifestError(manifest_path, e))?;

            modules.push(manifest)
        }

        Ok(modules)
    }

    pub async fn enabled_modules(
        &self,
    ) -> Result<Vec<InstalledModuleManifest>, EnabledModulesError> {
        let enabled_dir = &self.enable_dir;

        let mut read_dir = tokio::fs::read_dir(&enabled_dir)
            .await
            .map_err(|e| EnabledModulesError::DirIo(enabled_dir.clone(), e))?;

        let mut modules = Vec::new();

        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|e| EnabledModulesError::DirIo(enabled_dir.clone(), e))?
        {
            let Ok(module_name) = entry.file_name().into_string() else {
                continue;
            };

            let Ok(module_name) = ModuleName::try_from(module_name) else {
                continue;
            };

            if !tokio::fs::symlink_metadata(entry.path())
                .await
                .map(|md| md.is_symlink())
                .unwrap_or(false)
            {
                continue;
            }

            let manifest_path = self.module_dir(&module_name).join(MANIFEST_FILE_NAME);

            // the entry may still point at a manifest file rather than a module directory
            if !tokio::fs::try_exists(&manifest_path).await.unwrap_or(false) {
                self.migrate_legacy_manifest(&module_name).await;
            }

            if !tokio::fs::try_exists(&manifest_path)
                .await
                .map_err(|e| EnabledModulesError::DirIo(manifest_path.clone(), e))?
            {
                continue;
            }

            let manifest = read_manifest(&manifest_path)
                .await
                .map_err(|e| EnabledModulesError::ReadManifestError(manifest_path, e))?;

            modules.push(manifest)
        }

        Ok(modules)
    }

    pub async fn is_module_installed(
        &self,
        module_name: &ModuleName,
    ) -> Result<bool, IsModuleInstalledError> {
        self.migrate_legacy_manifest(module_name).await;

        self.find_manifest_file(module_name)
            .await
            .map(|path| path.is_some())
            .map_err(Into::into)
    }

    pub async fn is_module_enabled(
        &self,
        module_name: &ModuleName,
    ) -> Result<bool, IsModuleEnabledError> {
        let path = self.enable_dir.join(module_name.as_str());

        tokio::fs::symlink_metadata(&path)
            .await
            .map(|md| md.is_symlink())
            .or_else(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    Ok(false)
                } else {
                    Err(e.into())
                }
            })
    }

    pub async fn enable_module(&self, module_name: &ModuleName) -> Result<(), EnableError> {
        self.migrate_legacy_manifest(module_name).await;

        let module_dir = self.module_dir(module_name);

        if !tokio::fs::try_exists(&module_dir).await? {
            return Err(EnableError::NotInstalled);
        }

        let target_path = self.enable_target(module_name);
        let src_path = self.enable_dir.join(module_name.as_str());

        match create_symlink(&target_path, &src_path, true).await {
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                // disable and retry enabling one more time
                let _ = self.disable_module(module_name).await;

                create_symlink(&target_path, &src_path, true).await
            },
            result => result,
        }
        .map_err(Into::into)
    }

    pub async fn disable_module(&self, module_name: &ModuleName) -> Result<(), DisableError> {
        let path = self.enable_dir.join(module_name.as_str());

        let result = match tokio::fs::remove_file(&path).await {
            // on Windows a symlink to a directory has to be removed as a directory
            Err(err) if err.kind() != io::ErrorKind::NotFound => tokio::fs::remove_dir(&path).await,
            result => result,
        };

        result
            .or_else(|err| {
                if err.kind() == io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(err)
                }
            })
            .map_err(Into::into)
    }

    /// The path that the entry in [`Self::enable_dir`] points to for `module_name`.
    fn enable_target(&self, module_name: &ModuleName) -> PathBuf {
        if self
            .install_dir
            .parent()
            .zip(self.enable_dir.parent())
            .is_some_and(|(a, b)| a == b)
        {
            // install_dir and enable_dir share a parent, so point relatively: ../installed/<name>
            PathBuf::from("..")
                .join(self.install_dir.file_name().unwrap())
                .join(module_name.as_str())
        } else {
            self.module_dir(module_name)
        }
    }

    async fn find_manifest_file(
        &self,
        module_name: &ModuleName,
    ) -> Result<Option<PathBuf>, FindManifestError> {
        let path = self.module_dir(module_name).join(MANIFEST_FILE_NAME);

        match tokio::fs::try_exists(&path).await {
            Ok(true) => Ok(Some(path)),
            Ok(false) => Ok(None),
            Err(err) => Err(FindManifestError(path, err)),
        }
    }

    /// Moves the manifest of `module_name` into its own directory, if it is still a file directly
    /// in the install directory: `~/.asimov/modules/installed/<name>.{json,yaml,yml}`.
    async fn migrate_legacy_manifest(&self, module_name: &ModuleName) {
        let files = [
            format!("{module_name}.json"),
            format!("{module_name}.yaml"),
            format!("{module_name}.yml"),
        ];

        for file in &files {
            let path = self.install_dir.join(file);
            if tokio::fs::try_exists(&path).await.unwrap_or(false) {
                tracing::debug!(?path, "found a legacy manifest file, migrating...");

                self.move_legacy_manifest(module_name, &path).await;
            }
        }
    }

    async fn move_legacy_manifest(&self, module_name: &ModuleName, path: &Path) {
        let module_dir = self.module_dir(module_name);
        let dst = module_dir.join(MANIFEST_FILE_NAME);

        if tokio::fs::try_exists(&dst).await.unwrap_or(false) {
            tracing::debug!(
                legacy = ?path,
                current = ?dst,
                "module has both a legacy and a current manifest, removing the legacy one"
            );

            if let Err(err) = tokio::fs::remove_file(path).await {
                tracing::warn!(?path, ?err, "failed to remove the legacy manifest file");
            }

            return;
        }

        let result = async {
            let content = tokio::fs::read(path).await?;

            let manifest: InstalledModuleManifest =
                match path.extension().and_then(|ext| ext.to_str()) {
                    Some("yaml") | Some("yml") => {
                        serde_yaml_ng::from_slice(&content).map_err(io::Error::other)?
                    },
                    _ => serde_json::from_slice(&content).map_err(io::Error::other)?,
                };
            let content = serde_json::to_vec(&manifest).map_err(io::Error::other)?;

            tokio::fs::create_dir_all(&module_dir).await?;
            tokio::fs::write(&dst, content).await?;
            tokio::fs::remove_file(path).await
        }
        .await;

        if let Err(err) = result {
            tracing::debug!(
                from = ?path,
                to = ?dst,
                ?err,
                "tried to move module manifest from legacy path but failed"
            );
            return;
        }

        // the entry in `enable_dir` still points at the manifest file that just moved
        if self.is_module_enabled(module_name).await.unwrap_or(false) {
            let path = self.enable_dir.join(module_name.as_str());

            let _ = tokio::fs::remove_file(&path).await;
            let _ = create_symlink(&self.enable_target(module_name), &path, true).await;
        }
    }
}

#[cfg(unix)]
async fn create_symlink(target: &Path, link: &Path, _target_is_dir: bool) -> io::Result<()> {
    tokio::fs::symlink(target, link).await
}

#[cfg(windows)]
async fn create_symlink(target: &Path, link: &Path, target_is_dir: bool) -> io::Result<()> {
    if target_is_dir {
        tokio::fs::symlink_dir(target, link).await
    } else {
        tokio::fs::symlink_file(target, link).await
    }
}

/// Program names are joined onto [`Registry::exec_dir`], so a name that is not a plain path
/// component could reach files outside of it. Module names carry the same guarantee in their type.
fn is_valid_program_name(name: &str) -> bool {
    !name.is_empty()
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn manifest_file_module_name(path: &Path) -> Option<&str> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") | Some("yaml") | Some("yml") => {
            path.file_stem().and_then(|name| name.to_str())
        },
        _ => None,
    }
}

async fn read_manifest(
    path: impl AsRef<Path>,
) -> Result<InstalledModuleManifest, ReadManifestError> {
    let content = tokio::fs::read(&path)
        .await
        .map_err(ReadManifestError::InstalledManifestIo)?;

    Ok(serde_json::from_slice::<'_, InstalledModuleManifest>(
        &content,
    )?)
}
