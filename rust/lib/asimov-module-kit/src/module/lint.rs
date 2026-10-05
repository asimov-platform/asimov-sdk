// This is free and unencumbered software released into the public domain.

//! Consistency checks for an existing module directory.

use alloc::{format, string::String, vec::Vec};
use asimov_module::ModuleManifest;
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LintCode {
    /// The module name cannot be installed as an ASIMOV module.
    InvalidModuleName,
    /// No `.asimov/module.yaml` found.
    MissingManifest,
    /// `provides.programs` lists a program with no matching `[[bin]]`.
    ProgramNotInCargoToml,
    /// A `[[bin]]` is absent from `provides.programs`.
    BinNotInManifest,
    /// A program source file is unreferenced by any active `[[bin]]`.
    OrphanProgramSource,
    /// `handles:` is present but every field is empty.
    EmptyHandles,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LintFinding {
    pub severity: Severity,
    pub code: LintCode,
    pub message: String,
    pub path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct LintOptions {
    pub module_dir: PathBuf,
}

impl LintOptions {
    pub fn new(module_dir: impl Into<PathBuf>) -> Self {
        Self {
            module_dir: module_dir.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum LintError {
    #[error("not a module (no Cargo.toml found): {0}")]
    NotAModule(PathBuf),

    #[error("failed to parse `{0}`: {1}")]
    CargoToml(PathBuf, #[source] toml::de::Error),

    #[error("failed to parse `{0}`: {1}")]
    Manifest(PathBuf, #[source] serde_yaml_ng::Error),

    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(serde::Deserialize)]
struct CargoManifest {
    #[serde(default)]
    bin: Vec<CargoBin>,
}

#[derive(serde::Deserialize)]
struct CargoBin {
    name: String,
    path: Option<String>,
}

pub fn lint_module(options: LintOptions) -> Result<Vec<LintFinding>, LintError> {
    let module_dir = &options.module_dir;
    let cargo_toml_path = module_dir.join("Cargo.toml");
    if !cargo_toml_path.exists() {
        return Err(LintError::NotAModule(module_dir.clone()));
    }

    let cargo_toml: CargoManifest = toml::from_str(&fs::read_to_string(&cargo_toml_path)?)
        .map_err(|err| LintError::CargoToml(cargo_toml_path.clone(), err))?;

    let mut findings = Vec::new();

    let manifest_path = module_dir.join(".asimov/module.yaml");
    let manifest = if !manifest_path.exists() {
        findings.push(LintFinding {
            severity: Severity::Warning,
            code: LintCode::MissingManifest,
            message: "no `.asimov/module.yaml` found".into(),
            path: Some(manifest_path.clone()),
        });
        None
    } else {
        let manifest: ModuleManifest =
            serde_yaml_ng::from_str(&fs::read_to_string(&manifest_path)?)
                .map_err(|err| LintError::Manifest(manifest_path.clone(), err))?;
        Some(manifest)
    };

    if let Some(manifest) = &manifest {
        if let Err(err) = super::validate_module_name(&manifest.name) {
            findings.push(LintFinding {
                severity: Severity::Error,
                code: LintCode::InvalidModuleName,
                message: format!("{err}"),
                path: Some(manifest_path.clone()),
            });
        }

        for program in &manifest.provides.programs {
            if !cargo_toml.bin.iter().any(|bin| &bin.name == program) {
                findings.push(LintFinding {
                    severity: Severity::Error,
                    code: LintCode::ProgramNotInCargoToml,
                    message: format!(
                        "`provides.programs` lists `{program}`, but no matching `[[bin]]` was found in Cargo.toml"
                    ),
                    path: Some(manifest_path.clone()),
                });
            }
        }

        for bin in &cargo_toml.bin {
            if !manifest.provides.programs.iter().any(|p| p == &bin.name) {
                findings.push(LintFinding {
                    severity: Severity::Error,
                    code: LintCode::BinNotInManifest,
                    message: format!(
                        "`[[bin]]` `{}` is not listed in `provides.programs`",
                        bin.name
                    ),
                    path: Some(cargo_toml_path.clone()),
                });
            }
        }

        if manifest.handles.is_empty() {
            findings.push(LintFinding {
                severity: Severity::Warning,
                code: LintCode::EmptyHandles,
                message: "`handles:` is present but every field is empty".into(),
                path: Some(manifest_path.clone()),
            });
        }
    }

    let active_bin_paths: Vec<PathBuf> = cargo_toml
        .bin
        .iter()
        .filter_map(|bin| match &bin.path {
            Some(path) => Some(PathBuf::from(path)),
            // Cargo's implicit convention for a `[[bin]]` without an
            // explicit `path`: `src/bin/<name>.rs`, else `src/bin/<name>/main.rs`.
            None => {
                let flat = PathBuf::from(format!("src/bin/{}.rs", bin.name));
                if module_dir.join(&flat).exists() {
                    return Some(flat);
                }
                let nested = PathBuf::from(format!("src/bin/{}/main.rs", bin.name));
                module_dir.join(&nested).exists().then_some(nested)
            },
        })
        .collect();

    for source in find_program_sources(&module_dir.join("src")) {
        let relative = source
            .strip_prefix(module_dir)
            .unwrap_or(&source)
            .to_path_buf();
        if !active_bin_paths.contains(&relative) {
            findings.push(LintFinding {
                severity: Severity::Warning,
                code: LintCode::OrphanProgramSource,
                message: format!(
                    "`{}` is not referenced by any active `[[bin]]`",
                    relative.display()
                ),
                path: Some(source),
            });
        }
    }

    Ok(findings)
}

/// Finds `src/<kind>/main.rs` and `src/bin/*.rs` program source files.
fn find_program_sources(src_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(src_dir) else {
        return Vec::new();
    };

    let mut sources = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) == Some("bin") {
            let Ok(bin_entries) = fs::read_dir(&path) else {
                continue;
            };
            for bin_entry in bin_entries.flatten() {
                let bin_path = bin_entry.path();
                if bin_path.is_dir() {
                    // Nested convention: `src/bin/<name>/main.rs`.
                    let main_rs = bin_path.join("main.rs");
                    if main_rs.exists() {
                        sources.push(main_rs);
                    }
                } else if bin_path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    sources.push(bin_path);
                }
            }
        } else {
            let main_rs = path.join("main.rs");
            if main_rs.exists() {
                sources.push(main_rs);
            }
        }
    }
    sources
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(dir: &Path, relative: &str, contents: &str) {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn clean_module(dir: &Path) {
        write(
            dir,
            "Cargo.toml",
            r#"
[package]
name = "asimov-widget-module"

[[bin]]
name = "asimov-widget-emitter"
path = "src/emitter/main.rs"
"#,
        );
        write(dir, "src/emitter/main.rs", "fn main() {}");
        write(
            dir,
            ".asimov/module.yaml",
            r#"
name: widget
provides:
  programs:
    - asimov-widget-emitter
handles:
  url_protocols:
    - widget
  url_prefixes:
  url_patterns:
  file_extensions:
  content_types:
"#,
        );
    }

    #[test]
    fn clean_module_has_no_findings() {
        let dir = tempdir().unwrap();
        clean_module(dir.path());

        let findings = lint_module(LintOptions::new(dir.path())).unwrap();
        assert!(findings.is_empty(), "unexpected findings: {findings:#?}");
    }

    #[test]
    fn missing_manifest_is_a_warning() {
        let dir = tempdir().unwrap();
        clean_module(dir.path());
        fs::remove_dir_all(dir.path().join(".asimov")).unwrap();

        let findings = lint_module(LintOptions::new(dir.path())).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(findings[0].code, LintCode::MissingManifest);
    }

    #[test]
    fn empty_handles_is_a_warning() {
        let dir = tempdir().unwrap();
        clean_module(dir.path());
        write(
            dir.path(),
            ".asimov/module.yaml",
            r#"
name: widget
provides:
  programs:
    - asimov-widget-emitter
handles:
  url_protocols:
  url_prefixes:
  url_patterns:
  file_extensions:
  content_types:
"#,
        );

        let findings = lint_module(LintOptions::new(dir.path())).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(findings[0].code, LintCode::EmptyHandles);
    }

    #[test]
    fn program_missing_from_cargo_toml_is_an_error() {
        let dir = tempdir().unwrap();
        clean_module(dir.path());
        write(
            dir.path(),
            ".asimov/module.yaml",
            r#"
name: widget
provides:
  programs:
    - asimov-widget-emitter
    - asimov-widget-fetcher
handles:
  url_protocols:
    - widget
  url_prefixes:
  url_patterns:
  file_extensions:
  content_types:
"#,
        );

        let findings = lint_module(LintOptions::new(dir.path())).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
        assert_eq!(findings[0].code, LintCode::ProgramNotInCargoToml);
    }

    #[test]
    fn bin_missing_from_manifest_is_an_error() {
        let dir = tempdir().unwrap();
        clean_module(dir.path());
        write(
            dir.path(),
            "Cargo.toml",
            r#"
[package]
name = "asimov-widget-module"

[[bin]]
name = "asimov-widget-emitter"
path = "src/emitter/main.rs"

[[bin]]
name = "asimov-widget-fetcher"
path = "src/fetcher/main.rs"
"#,
        );
        write(dir.path(), "src/fetcher/main.rs", "fn main() {}");

        let findings = lint_module(LintOptions::new(dir.path())).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
        assert_eq!(findings[0].code, LintCode::BinNotInManifest);
    }

    #[test]
    fn orphan_program_source_is_a_warning() {
        let dir = tempdir().unwrap();
        clean_module(dir.path());
        write(dir.path(), "src/fetcher/main.rs", "fn main() {}");

        let findings = lint_module(LintOptions::new(dir.path())).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(findings[0].code, LintCode::OrphanProgramSource);
    }

    #[test]
    fn flat_bin_convention_is_recognized() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "Cargo.toml",
            r#"
[package]
name = "asimov-widget-module"

[[bin]]
name = "asimov-widget-emitter"
path = "src/bin/emitter.rs"
"#,
        );
        write(dir.path(), "src/bin/emitter.rs", "fn main() {}");
        write(
            dir.path(),
            ".asimov/module.yaml",
            r#"
name: widget
provides:
  programs:
    - asimov-widget-emitter
handles:
  url_protocols:
    - widget
  url_prefixes:
  url_patterns:
  file_extensions:
  content_types:
"#,
        );

        let findings = lint_module(LintOptions::new(dir.path())).unwrap();
        assert!(findings.is_empty(), "unexpected findings: {findings:#?}");
    }

    #[test]
    fn bin_without_explicit_path_is_not_a_false_positive_orphan() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "Cargo.toml",
            r#"
[package]
name = "asimov-widget-module"

[[bin]]
name = "asimov-widget-emitter"
"#,
        );
        // No `path` given: Cargo's implicit convention is `src/bin/<name>.rs`.
        write(
            dir.path(),
            "src/bin/asimov-widget-emitter.rs",
            "fn main() {}",
        );
        write(
            dir.path(),
            ".asimov/module.yaml",
            r#"
name: widget
provides:
  programs:
    - asimov-widget-emitter
handles:
  url_protocols:
    - widget
  url_prefixes:
  url_patterns:
  file_extensions:
  content_types:
"#,
        );

        let findings = lint_module(LintOptions::new(dir.path())).unwrap();
        assert!(findings.is_empty(), "unexpected findings: {findings:#?}");
    }

    #[test]
    fn nested_bin_convention_orphan_is_detected() {
        let dir = tempdir().unwrap();
        clean_module(dir.path());
        // Unreferenced, using the nested `src/bin/<name>/main.rs` convention.
        write(dir.path(), "src/bin/fetcher/main.rs", "fn main() {}");

        let findings = lint_module(LintOptions::new(dir.path())).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(findings[0].code, LintCode::OrphanProgramSource);
    }

    #[test]
    fn not_a_module_errors() {
        let dir = tempdir().unwrap();
        let err = lint_module(LintOptions::new(dir.path())).unwrap_err();
        assert!(matches!(err, LintError::NotAModule(_)));
    }
}
