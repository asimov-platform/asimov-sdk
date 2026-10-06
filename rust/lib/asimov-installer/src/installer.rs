// This is free and unencumbered software released into the public domain.

use alloc::format;
use asimov_module::{InstalledModuleManifest, ModuleManifest, ModuleName, tracing};
use std::{
    boxed::Box,
    path::{Path, PathBuf},
    string::String,
};

pub mod error;
use error::*;

use asimov_registry::Registry;

mod github;
mod platform;

#[derive(Clone, Debug)]
pub struct Installer {
    client: reqwest::Client,
    registry: Registry,
}

impl Default for Installer {
    fn default() -> Self {
        // TODO: retry support
        let client = reqwest::Client::builder()
            .user_agent("asimov-module-installer")
            .connect_timeout(std::time::Duration::from_secs(10))
            .read_timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");
        let registry = Registry::default();
        Self::new(client, registry)
    }
}

#[derive(Clone, Debug, Default, bon::Builder)]
#[builder(on(String, into))]
pub struct InstallOptions {
    pub version: Option<String>,
    pub model_size: Option<String>,
}

#[derive(Clone, Debug)]
struct Preinstalled {
    module_name: ModuleName,
    manifest: ModuleManifest,
    version: String,
    readme: Option<String>,
    extract_dir: PathBuf,
}

impl Installer {
    pub fn new(client: reqwest::Client, registry: Registry) -> Self {
        Self { client, registry }
    }

    /// ```rust,no_run
    /// # use asimov_installer::{Installer, InstallOptions};
    /// let i = Installer::default();
    /// let module = "foobar".parse().unwrap();
    /// i.install_module(&module, &InstallOptions::default());
    /// ```
    pub async fn install_module(
        &self,
        module_name: &ModuleName,
        options: &InstallOptions,
    ) -> Result<(), InstallError> {
        let work_dir = self.work_dir(module_name).await?;

        let preinstalled = self
            .preinstall(module_name, options, work_dir.path())
            .await?;

        self.finish_install(preinstalled, work_dir.path()).await?;

        Ok(())
    }

    pub async fn fetch_latest_release(
        &self,
        module_name: &ModuleName,
    ) -> Result<String, FetchError> {
        github::fetch_latest_release(&self.client, module_name).await
    }

    /// Stage a replacement before publishing it, preserving enabled state.
    ///
    /// Publication failures roll back the prior installation. An interrupted
    /// publication is recovered on the next upgrade attempt. Callers must exclude
    /// concurrent install/uninstall and enable/disable operations on the registry.
    ///
    /// ```rust,no_run
    /// # use asimov_installer::{Installer, InstallOptions};
    /// let i = Installer::default();
    /// let module = "foobar".parse().unwrap();
    /// i.upgrade_module(&module, &InstallOptions::default());
    /// ```
    pub async fn upgrade_module(
        &self,
        module_name: &ModuleName,
        options: &InstallOptions,
    ) -> Result<(), UpgradeError> {
        self.registry.recover_upgrade(module_name).await?;
        let version = if let Some(ref want_version) = options.version {
            want_version.clone()
        } else {
            self.fetch_latest_release(module_name).await?
        };

        let current_version = self.registry.module_version(module_name).await?;
        match current_version {
            Some(current) if current == version => return Ok(()),
            Some(_) => (),
            None => tracing::debug!(%module_name, "installed module does not define a version"),
        };

        let work_dir = self.work_dir(module_name).await?;

        let options = InstallOptions {
            version: Some(version),
            model_size: options.model_size.clone(),
        };
        let preinstalled = self
            .preinstall(module_name, &options, work_dir.path())
            .await?;

        let (_, staged) = self.stage_install(preinstalled, work_dir.path()).await?;
        self.registry.replace_module(module_name, staged).await?;

        Ok(())
    }

    pub async fn uninstall_module(&self, module_name: &ModuleName) -> Result<(), UninstallError> {
        let manifest = self.registry.read_manifest(module_name).await?;

        self.registry.disable_module(module_name).await?;

        for program in &manifest.manifest.provides.programs {
            self.registry
                .remove_binary(program)
                .await
                .map_err(|e| UninstallError::RemoveBinary(program.into(), e))?;
        }

        self.registry.remove_module(module_name).await?;

        Ok(())
    }

    async fn work_dir(&self, module_name: &ModuleName) -> Result<tempfile::TempDir, WorkDirError> {
        self.registry
            .create_file_tree()
            .await
            .map_err(WorkDirError::CreateFileTree)?;

        let install_dir = self.registry.install_dir();

        tempfile::Builder::new()
            .prefix(&format!(".{module_name}-"))
            .tempdir_in(install_dir.parent().unwrap_or(install_dir))
            .map_err(WorkDirError::CreateDir)
    }

    async fn preinstall(
        &self,
        module_name: &ModuleName,
        options: &InstallOptions,
        temp_dir: &Path,
    ) -> Result<Preinstalled, PreinstallError> {
        let platform = platform::detect_platform();

        let version = if let Some(ref want_version) = options.version {
            want_version.clone()
        } else {
            github::fetch_latest_release(&self.client, module_name)
                .await
                .map_err(PreinstallError::FetchRelease)?
        };

        let manifest = github::fetch_module_manifest(&self.client, module_name, &version)
            .await
            .map_err(PreinstallError::FetchManifest)?;

        let (asset_url, download_path) = github::download_matching_asset(
            &self.client,
            module_name,
            &version,
            &platform,
            temp_dir,
        )
        .await?;

        // pass the model_size option to dependencies
        let options = InstallOptions::builder()
            .maybe_model_size(options.model_size.clone())
            .build();
        let subdeps = &manifest.requires.modules;
        for module in subdeps {
            let module = ModuleName::try_from(module.clone())
                .map_err(PreinstallError::InvalidDependencyName)?;

            if self
                .registry
                .is_module_installed(&module)
                .await
                .unwrap_or(false)
            {
                continue;
            }
            Box::pin(self.install_module(&module, &options))
                .await
                .map_err(|e| PreinstallError::Dependency(module.into_string(), Box::new(e)))?;
        }

        match github::fetch_checksum(&self.client, &asset_url).await {
            Ok(None) => {},
            Ok(Some(checksum)) => {
                github::verify_checksum(&download_path, &checksum).await?;
            },
            Err(err) => Err(err)?,
        }

        let extract_dir = temp_dir.join("extract");
        tokio::fs::create_dir(&extract_dir)
            .await
            .map_err(PreinstallError::CreateExtractDir)?;

        github::extract_files(&download_path, &extract_dir)
            .await
            .map_err(PreinstallError::Extract)?;

        for (name, model) in &manifest.requires.models {
            let Some(repo) = name.strip_prefix("hf:") else {
                tracing::debug!(
                    ?name,
                    "unexpected format for required model, only `hf:<user>/<repo>` is supported"
                );
                continue;
            };

            use asimov_module::RequiredModel;
            let filename = match (model, &options.model_size) {
                (RequiredModel::Url(url), None | Some(_)) => url,
                (RequiredModel::Choices(choices), None) => {
                    if choices
                        .iter()
                        .any(|(_, url)| asimov_huggingface::file_exists(repo, url).is_some())
                    {
                        // user didn't specify a model size/version to install
                        // and one of the choices is already installed
                        continue;
                    }
                    let Some((_, model)) = choices.first() else {
                        // malformed manifest?
                        tracing::warn!(
                            ?module_name,
                            "manifest defines required models with no choices"
                        );
                        continue;
                    };
                    model
                },
                (RequiredModel::Choices(choices), Some(want_model)) => {
                    &choices
                        .iter()
                        .find(|(name, _)| *name == *want_model)
                        .ok_or_else(|| PreinstallError::NoSuchModel(want_model.clone()))?
                        .1
                },
            };

            asimov_huggingface::ensure_file(repo, filename)?;
        }

        let readme = match find_readme(&extract_dir).await {
            Some(readme) => Some(readme),
            None => github::fetch_readme(&self.client, module_name, &version).await,
        };

        Ok(Preinstalled {
            module_name: ModuleName::try_from(manifest.name.clone())
                .map_err(PreinstallError::InvalidModuleName)?,
            manifest,
            version,
            readme,
            extract_dir,
        })
    }

    async fn finish_install(
        &self,
        preinstalled: Preinstalled,
        work_dir: &Path,
    ) -> Result<(), FinishInstallError> {
        let (module_name, module_dir) = self.stage_install(preinstalled, work_dir).await?;
        self.registry
            .add_module(&module_name, module_dir.path())
            .await?;
        Ok(())
    }

    async fn stage_install(
        &self,
        preinstalled: Preinstalled,
        work_dir: &Path,
    ) -> Result<(ModuleName, tempfile::TempDir), FinishInstallError> {
        let Preinstalled {
            module_name,
            manifest,
            version,
            readme,
            extract_dir,
        } = preinstalled;

        // A sibling survives cancellation of the download work directory while
        // the registry's publication worker owns the staged module.
        let parent = work_dir.parent().unwrap_or(work_dir);
        let module_dir = tempfile::Builder::new()
            .prefix(".module-")
            .tempdir_in(parent)
            .map_err(|e| FinishInstallError::CreateDir(parent.into(), e))?;

        assemble_module(
            module_dir.path(),
            InstalledModuleManifest {
                version: Some(version),
                manifest,
            },
            readme,
            &extract_dir,
        )
        .await?;

        Ok((module_name, module_dir))
    }
}

async fn assemble_module(
    module_dir: &Path,
    manifest: InstalledModuleManifest,
    readme: Option<String>,
    extract_dir: &Path,
) -> Result<(), FinishInstallError> {
    #[cfg(unix)]
    {
        use std::fs::Permissions;
        use std::os::unix::fs::PermissionsExt;

        tokio::fs::set_permissions(module_dir, Permissions::from_mode(0o755))
            .await
            .map_err(FinishInstallError::SetPermissions)?;
    }

    let bin_dir = module_dir.join(asimov_registry::BIN_DIR_NAME);

    tokio::fs::create_dir_all(&bin_dir)
        .await
        .map_err(|e| FinishInstallError::CreateDir(bin_dir.clone(), e))?;

    for program in &manifest.manifest.provides.programs {
        let src = extract_dir.join(program);

        // On Windows add the .exe extension to the binary name:
        #[cfg(windows)]
        let src = src.with_extension("exe");

        let dst = bin_dir.join(program);

        tokio::fs::rename(&src, &dst)
            .await
            .map_err(|e| FinishInstallError::MoveBinary(program.clone(), e))?;

        #[cfg(unix)]
        {
            use std::fs::Permissions;
            use std::os::unix::fs::PermissionsExt;

            tokio::fs::set_permissions(&dst, Permissions::from_mode(0o755))
                .await
                .map_err(FinishInstallError::SetPermissions)?;
        }
    }

    if let Some(readme) = readme {
        let readme_path = module_dir.join(asimov_registry::README_FILE_PATH);
        let doc_dir = readme_path.parent().unwrap();

        tokio::fs::create_dir_all(doc_dir)
            .await
            .map_err(|e| FinishInstallError::CreateDir(doc_dir.into(), e))?;

        tokio::fs::write(&readme_path, readme)
            .await
            .map_err(|e| FinishInstallError::WriteFile(readme_path, e))?;
    }

    let manifest_path = module_dir.join(asimov_registry::MANIFEST_FILE_NAME);

    let serialized = serde_json::to_vec_pretty(&manifest).map_err(FinishInstallError::Serialize)?;

    tokio::fs::write(&manifest_path, serialized)
        .await
        .map_err(|e| FinishInstallError::WriteFile(manifest_path, e))
}

async fn find_readme(extract_dir: &Path) -> Option<String> {
    let mut entries = tokio::fs::read_dir(extract_dir).await.ok()?;

    while let Ok(Some(entry)) = entries.next_entry().await {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };

        if !file_name.to_ascii_uppercase().starts_with("README") {
            continue;
        }

        if !entry
            .file_type()
            .await
            .map(|file_type| file_type.is_file())
            .unwrap_or(false)
        {
            continue;
        }

        return tokio::fs::read_to_string(entry.path()).await.ok();
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn failed_replacement_assembly_preserves_the_installed_module() {
        let root = tempfile::tempdir().unwrap();
        let registry = Registry::new(root.path(), Default::default());
        registry.create_file_tree().await.unwrap();
        let installer = Installer::new(reqwest::Client::new(), registry);
        let name: ModuleName = "example".parse().unwrap();
        let old = root.path().join("old");
        std::fs::create_dir_all(old.join("bin")).unwrap();
        std::fs::write(old.join("bin/example"), "working binary").unwrap();
        std::fs::write(
            old.join("manifest.json"),
            r#"{"name":"example","version":"1","provides":{"programs":["example"]}}"#,
        )
        .unwrap();
        installer.registry.add_module(&name, old).await.unwrap();
        installer.registry.enable_module(&name).await.unwrap();

        let work = installer.work_dir(&name).await.unwrap();
        let extract = work.path().join("extract");
        std::fs::create_dir(&extract).unwrap();
        let manifest = installer
            .registry
            .read_manifest(&name)
            .await
            .unwrap()
            .manifest;
        let replacement = Preinstalled {
            module_name: name.clone(),
            manifest,
            version: "2".into(),
            readme: None,
            extract_dir: extract,
        };
        assert!(matches!(
            installer.stage_install(replacement, work.path()).await,
            Err(FinishInstallError::MoveBinary(_, _))
        ));
        assert_eq!(
            installer
                .registry
                .module_version(&name)
                .await
                .unwrap()
                .as_deref(),
            Some("1")
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("libexec/example")).unwrap(),
            "working binary"
        );
        assert!(installer.registry.is_module_enabled(&name).await.unwrap());
    }

    #[tokio::test]
    async fn find_readme_in_archive() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(find_readme(dir.path()).await, None);

        std::fs::create_dir(dir.path().join("README")).unwrap();
        assert_eq!(find_readme(dir.path()).await, None);

        std::fs::write(dir.path().join("readme.md"), "# Hello").unwrap();
        assert_eq!(find_readme(dir.path()).await.as_deref(), Some("# Hello"));
    }
}
