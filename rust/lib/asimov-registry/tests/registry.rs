use std::path::PathBuf;

use asimov_module::InstalledModuleManifest;
use asimov_registry::{Registry, error::ReadReadmeError};
use tempfile::tempdir;

#[tokio::test]
async fn module_readers_find_a_registry_created_installation() {
    use asimov_module::{ModuleManifest, resolve::Resolver};
    let root = tempdir().unwrap();
    let registry = Registry::new(root.path(), Default::default());
    registry.create_file_tree().await.unwrap();
    let staged = tempdir().unwrap();
    std::fs::create_dir(staged.path().join("bin")).unwrap();
    std::fs::write(staged.path().join("bin/asimov-ipfs-fetcher"), "fixture").unwrap();
    let installed = InstalledModuleManifest {
        version: Some("1.2.3".into()),
        manifest: serde_json::from_str(SAMPLE_MANIFEST).unwrap(),
    };
    std::fs::write(
        staged.path().join(asimov_registry::MANIFEST_FILE_NAME),
        serde_json::to_vec(&installed).unwrap(),
    )
    .unwrap();
    let name = "ipfs".parse().unwrap();
    registry.add_module(&name, staged.path()).await.unwrap();
    assert_eq!(
        ModuleManifest::read_manifest_from_root("ipfs", root.path()).unwrap(),
        installed.manifest
    );
    let resolver = Resolver::try_from_dir(registry.install_dir()).unwrap();
    let modules = resolver.resolve("ipfs://example").unwrap();
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].name, "ipfs");
}

// See: https://asimov-specs.github.io/module-manifest/
const SAMPLE_MANIFEST: &str = r#"{
  "name": "ipfs",
  "label": "IPFS",
  "title": "ASIMOV IPFS Module",
  "summary": "IPFS protocol support.",
  "links": [
    "https://github.com/asimov-modules/asimov-ipfs-module",
    "https://crates.io/crates/asimov-ipfs-module"
  ],
  "provides": {
    "programs": ["asimov-ipfs-fetcher"]
  },
  "handles": {
    "url_protocols": ["ipfs"]
  }
}
"#;

/// Manifests were YAML back when they lived directly in the install directory.
const LEGACY_SAMPLE_MANIFEST: &str = r#"# See: https://asimov-specs.github.io/module-manifest/
---
name: ipfs
label: IPFS
title: ASIMOV IPFS Module
summary: IPFS protocol support.
links:
  - https://github.com/asimov-modules/asimov-ipfs-module
  - https://crates.io/crates/asimov-ipfs-module

provides:
  programs:
    - asimov-ipfs-fetcher

handles:
  url_protocols:
    - ipfs
  url_prefixes:
  url_patterns:
  file_extensions:
  content_types:
"#;

pub fn compare_manifest(a: &InstalledModuleManifest, b: &InstalledModuleManifest) {
    assert_eq!(a.manifest.name, b.manifest.name);
    assert_eq!(a.manifest.label, b.manifest.label);
    assert_eq!(a.manifest.summary, b.manifest.summary);
    assert_eq!(a.manifest.links, b.manifest.links);
    assert_eq!(a.manifest.provides.programs, b.manifest.provides.programs);
    assert_eq!(
        a.manifest.handles.url_protocols,
        b.manifest.handles.url_protocols
    );
    assert_eq!(
        a.manifest.handles.url_prefixes,
        b.manifest.handles.url_prefixes
    );
    assert_eq!(
        a.manifest.handles.url_patterns,
        b.manifest.handles.url_patterns
    );
    assert_eq!(
        a.manifest.handles.file_extensions,
        b.manifest.handles.file_extensions
    );
    assert_eq!(
        a.manifest.handles.content_types,
        b.manifest.handles.content_types
    );
}

pub async fn test_registry(
    registry: Registry,
    module_dir: PathBuf,
    enabled_path: PathBuf,
    is_relative: bool,
) {
    let sample = "sample".parse().unwrap();

    registry.create_file_tree().await.unwrap();
    assert_eq!(registry.installed_modules().await.unwrap().len(), 0);

    tokio::fs::create_dir_all(&module_dir).await.unwrap();
    tokio::fs::write(module_dir.join("manifest.json"), SAMPLE_MANIFEST)
        .await
        .unwrap();

    let module = registry.read_manifest(&sample).await.unwrap();
    assert_eq!(module.manifest.name, "ipfs");
    assert_eq!(module.manifest.label.as_deref(), Some("IPFS"));
    assert_eq!(
        module.manifest.summary.as_deref(),
        Some("IPFS protocol support.")
    );

    let installed_modules = registry.installed_modules().await.unwrap();
    assert_eq!(installed_modules.len(), 1);
    compare_manifest(&installed_modules[0], &module);

    assert_eq!(registry.enabled_modules().await.unwrap().len(), 0);
    registry.enable_module(&sample).await.unwrap();

    let enabled_modules = registry.enabled_modules().await.unwrap();
    assert_eq!(enabled_modules.len(), 1);
    compare_manifest(&enabled_modules[0], &module);

    let metadata = std::fs::symlink_metadata(&enabled_path).unwrap();
    assert!(metadata.is_symlink());

    let link_path = std::fs::read_link(&enabled_path).unwrap();
    assert_eq!(link_path.starts_with("../"), is_relative);

    let absolute_path = std::fs::canonicalize(enabled_path.parent().unwrap().join(link_path));
    assert_eq!(
        absolute_path.unwrap(),
        std::fs::canonicalize(&module_dir).unwrap()
    );

    assert_eq!(registry.read_readme(&sample).await.unwrap(), None);
    tokio::fs::create_dir_all(module_dir.join("doc"))
        .await
        .unwrap();
    tokio::fs::write(module_dir.join("doc/README.md"), "# Sample\n")
        .await
        .unwrap();
    assert_eq!(
        registry.read_readme(&sample).await.unwrap().as_deref(),
        Some("# Sample\n")
    );

    registry.remove_module(&sample).await.unwrap();
    assert!(!module_dir.exists());
    assert_eq!(registry.installed_modules().await.unwrap().len(), 0);

    assert!(matches!(
        registry.read_readme(&sample).await,
        Err(ReadReadmeError::NotInstalled)
    ));
}

#[tokio::test]
pub async fn test_default_registry() {
    let base_dir = tempdir().unwrap();
    let registry = Registry::new(base_dir.path(), Default::default());

    let module_dir = base_dir.path().join("modules/installed/sample");
    let enabled_path = base_dir.path().join("modules/enabled/sample");

    test_registry(registry, module_dir, enabled_path, true).await;
}

#[tokio::test]
pub async fn test_custom_registry() {
    let base_dir = tempdir().unwrap();
    let module_dir = base_dir.path().join("a/b/c/sample");
    let enabled_path = base_dir.path().join("b/sample");
    let libexec_path = base_dir.path().join("c");

    let registry = Registry::with_dirs(
        module_dir.parent().unwrap(),
        enabled_path.parent().unwrap(),
        libexec_path,
        Default::default(),
    );
    test_registry(registry, module_dir, enabled_path, false).await;
}

#[tokio::test]
pub async fn test_migrate_legacy_layout() {
    let base_dir = tempdir().unwrap();
    let registry = Registry::new(base_dir.path(), Default::default());
    registry.create_file_tree().await.unwrap();

    let sample = "sample".parse().unwrap();

    let legacy_path = base_dir.path().join("modules/installed/sample.yaml");
    tokio::fs::write(&legacy_path, LEGACY_SAMPLE_MANIFEST)
        .await
        .unwrap();

    assert!(registry.is_module_installed(&sample).await.unwrap());
    registry.enable_module(&sample).await.unwrap();

    let module_dir = base_dir.path().join("modules/installed/sample");
    assert!(!legacy_path.exists());
    assert!(module_dir.join("manifest.json").is_file());
    assert_eq!(registry.installed_modules().await.unwrap().len(), 1);
    assert_eq!(registry.enabled_modules().await.unwrap().len(), 1);

    let enabled_path = base_dir.path().join("modules/enabled/sample");
    let link_path = std::fs::read_link(&enabled_path).unwrap();
    assert_eq!(
        std::fs::canonicalize(enabled_path.parent().unwrap().join(link_path)).unwrap(),
        std::fs::canonicalize(&module_dir).unwrap()
    );
}

#[tokio::test]
pub async fn test_legacy_manifest_never_replaces_a_current_one() {
    let manifest_json = |version: &str| {
        let manifest = InstalledModuleManifest {
            version: Some(version.into()),
            manifest: serde_json::from_str(SAMPLE_MANIFEST).unwrap(),
        };
        serde_json::to_vec(&manifest).unwrap()
    };

    let base_dir = tempdir().unwrap();
    let registry = Registry::new(base_dir.path(), Default::default());
    registry.create_file_tree().await.unwrap();

    let sample = "sample".parse().unwrap();

    let module_dir = base_dir.path().join("modules/installed/sample");
    tokio::fs::create_dir(&module_dir).await.unwrap();
    tokio::fs::write(module_dir.join("manifest.json"), manifest_json("2.0.0"))
        .await
        .unwrap();

    let legacy_path = base_dir.path().join("modules/installed/sample.json");
    tokio::fs::write(&legacy_path, manifest_json("0.1.7"))
        .await
        .unwrap();

    assert_eq!(
        registry.module_version(&sample).await.unwrap().as_deref(),
        Some("2.0.0")
    );
    assert!(!legacy_path.exists());
    assert_eq!(registry.installed_modules().await.unwrap().len(), 1);
}

#[tokio::test]
pub async fn test_migrate_legacy_version() {
    let base_dir = tempdir().unwrap();
    let registry = Registry::new(base_dir.path(), Default::default());
    registry.create_file_tree().await.unwrap();

    let sample = "sample".parse().unwrap();

    let legacy_path = base_dir.path().join("modules/installed/sample.json");
    let manifest = InstalledModuleManifest {
        version: Some("0.1.7".into()),
        manifest: serde_json::from_str(SAMPLE_MANIFEST).unwrap(),
    };
    tokio::fs::write(&legacy_path, serde_json::to_vec(&manifest).unwrap())
        .await
        .unwrap();

    assert_eq!(
        registry.module_version(&sample).await.unwrap().as_deref(),
        Some("0.1.7")
    );
    assert!(
        base_dir
            .path()
            .join("modules/installed/sample/manifest.json")
            .is_file()
    );
}

#[tokio::test]
async fn migration_preserves_dependencies_and_model_choice_order() {
    let yaml = r#"
name: sample
version: '1.2.3'
requires:
  modules: [other]
  platforms: [linux]
  programs: [python3]
  libraries: [libssl]
  models:
    hf:example/direct: model.bin
    hf:example/choices:
      small: small.bin
      medium: medium.bin
      large: large.bin
  datasets: [dataset]
  ontologies: [ontology]
  classes: [class]
  datatypes: [datatype]
"#;
    let expected: InstalledModuleManifest = serde_yaml_ng::from_str(yaml).unwrap();
    assert_eq!(
        expected.manifest.requires.models["hf:example/choices"],
        asimov_module::RequiredModel::Choices(vec![
            ("small".into(), "small.bin".into()),
            ("medium".into(), "medium.bin".into()),
            ("large".into(), "large.bin".into()),
        ])
    );

    for extension in ["yaml", "yml"] {
        let base_dir = tempdir().unwrap();
        let registry = Registry::new(base_dir.path(), Default::default());
        registry.create_file_tree().await.unwrap();
        let sample = "sample".parse().unwrap();
        let legacy = registry.install_dir().join(format!("sample.{extension}"));
        tokio::fs::write(&legacy, yaml).await.unwrap();

        let migrated = registry.read_manifest(&sample).await.unwrap();
        assert_eq!(migrated.manifest, expected.manifest);
        assert_eq!(migrated.version, expected.version);
        assert!(!legacy.exists());

        let json = tokio::fs::read(registry.module_dir(&sample).join("manifest.json"))
            .await
            .unwrap();
        let installed: InstalledModuleManifest = serde_json::from_slice(&json).unwrap();
        assert_eq!(installed.manifest, expected.manifest);
        assert_eq!(installed.version, expected.version);
    }
}
