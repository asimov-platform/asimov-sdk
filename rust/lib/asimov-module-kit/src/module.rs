// This is free and unencumbered software released into the public domain.

//! Module authoring support.

use alloc::{boxed::Box, format, string::String, vec::Vec};
use cargo_generate::{GenerateArgs, TemplatePath, Vcs, generate};
use git2::build::RepoBuilder;
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};
use thiserror::Error;

pub mod cargo_toml;
#[cfg(feature = "lint")]
pub mod lint;
pub mod manifest_edit;
pub mod program;

pub const DEFAULT_TEMPLATE_GIT: &str = "https://github.com/asimov-modules/asimov-template-module";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TemplateSource {
    Git(String),
    Path(PathBuf),
}

impl Default for TemplateSource {
    fn default() -> Self {
        Self::Git(DEFAULT_TEMPLATE_GIT.into())
    }
}

#[derive(Clone, Debug)]
pub struct NewModuleOptions {
    pub target_dir: PathBuf,
    pub name: String,
    pub template: TemplateSource,
    pub branch: Option<String>,
    pub package_name: Option<String>,
    pub package_description: Option<String>,
    pub package_authors: Vec<String>,
    pub module_label: Option<String>,
    pub module_title: Option<String>,
    pub module_summary: Option<String>,
    pub program_name: Option<String>,
    /// Additional programs to scaffold beyond `program_name`, added the same
    /// way [`program::add_program`] would, right after the module itself is
    /// generated.
    pub extra_programs: Vec<String>,
    pub repository_url: Option<String>,
    pub asimov_version: Option<String>,
    pub publish: bool,
    pub vcs: Option<String>,
    /// Whether to scaffold an initial program at all. Defaults to `true`;
    /// set to `false` (via [`Self::without_program`]) to create a bare
    /// module with no programs — add them later via [`program::add_program`].
    pub create_program: bool,
}

impl NewModuleOptions {
    pub fn new(target_dir: impl Into<PathBuf>, name: impl Into<String>) -> Self {
        Self {
            target_dir: target_dir.into(),
            name: name.into(),
            template: TemplateSource::default(),
            branch: None,
            package_name: None,
            package_description: None,
            package_authors: Vec::new(),
            module_label: None,
            module_title: None,
            module_summary: None,
            program_name: None,
            extra_programs: Vec::new(),
            repository_url: None,
            asimov_version: Some(env!("CARGO_PKG_VERSION").into()),
            publish: false,
            vcs: Some("none".into()),
            create_program: true,
        }
    }

    pub fn template_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.template = TemplateSource::Path(path.into());
        self
    }

    pub fn template_git(mut self, git: impl Into<String>) -> Self {
        self.template = TemplateSource::Git(git.into());
        self
    }

    /// Creates a bare module with no initial program.
    pub fn without_program(mut self) -> Self {
        self.create_program = false;
        self
    }

    /// Sets the module's initial set of programs. Given an empty list, the
    /// template's own default program is used, unchanged (today's
    /// `asimov-<name>-emitter`). Given one or more names, the default is
    /// skipped entirely and *every* name is added the same way
    /// [`program::add_program`] would, right after the template is
    /// generated — there is no special-cased "first" program.
    pub fn programs(mut self, names: impl IntoIterator<Item = String>) -> Self {
        let names: Vec<String> = names.into_iter().collect();
        if names.is_empty() {
            return self;
        }
        self.create_program = false;
        self.program_name = None;
        self.extra_programs = names;
        self
    }
}

#[derive(Debug, Error)]
pub enum NewModuleError {
    #[error("module name must not be empty")]
    EmptyName,

    #[error(
        "module name `{0}` is not supported; use at most 64 lowercase letters, digits, and hyphens, starting with a letter and ending without a hyphen"
    )]
    InvalidName(String),

    #[error("target directory has no parent: {0}")]
    MissingTargetParent(PathBuf),

    #[error("target directory has no final path component: {0}")]
    MissingTargetName(PathBuf),

    #[error("target directory already exists: {0}")]
    TargetExists(PathBuf),

    #[error("failed to create target parent directory `{0}`: {1}")]
    CreateTargetParent(PathBuf, #[source] io::Error),

    #[error("failed to run `cargo generate`: {0}")]
    CargoGenerate(#[source] anyhow::Error),

    #[error(transparent)]
    InvalidProgramName(#[from] InvalidProgramName),

    #[error("failed to add program `{0}`: {1}")]
    AddProgram(String, #[source] Box<program::AddProgramError>),

    #[error("failed to create a temporary directory: {0}")]
    TempDir(#[source] io::Error),

    #[error("failed to clone template `{0}`: {1}")]
    CloneTemplate(String, #[source] git2::Error),
}

/// The program name doesn't match the `asimov-<module>-<kind>` shape.
///
/// The `<kind>` segment itself is never checked against any fixed list —
/// any lowercase, hyphenated word is accepted; only the overall shape
/// (matching the naming convention already enforced for module names) is
/// validated here.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[error(
    "program name `{0}` is not supported; use the shape `asimov-<module>-<kind>` (lowercase letters, digits, and hyphens)"
)]
pub struct InvalidProgramName(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatedModule {
    pub module_name: String,
    pub crate_name: String,
    /// The names of the programs scaffolded, in the order they were added.
    /// Empty if the module was created bare.
    pub program_names: Vec<String>,
    pub target_dir: PathBuf,
}

pub fn new_module(options: NewModuleOptions) -> Result<CreatedModule, NewModuleError> {
    #[cfg(feature = "tracing")]
    tracing::info!(
        module = %options.name,
        target = ?options.target_dir,
        template = ?options.template,
        "generating new ASIMOV module"
    );

    validate_module_name(&options.name)?;
    let expected_program_prefix = format!("asimov-{}-", options.name);
    let mut programs_to_create: Vec<String> = Vec::new();
    if let Some(program_name) = &options.program_name {
        validate_program_name(program_name)?;
        if !program_name.starts_with(&expected_program_prefix) {
            return Err(NewModuleError::InvalidProgramName(InvalidProgramName(
                program_name.clone(),
            )));
        }
        if !programs_to_create.contains(program_name) {
            programs_to_create.push(program_name.clone());
        }
    }
    for extra_program in &options.extra_programs {
        validate_program_name(extra_program)?;
        if !extra_program.starts_with(&expected_program_prefix) {
            return Err(NewModuleError::InvalidProgramName(InvalidProgramName(
                extra_program.clone(),
            )));
        }
        if !programs_to_create.contains(extra_program) {
            programs_to_create.push(extra_program.clone());
        }
    }
    if programs_to_create.is_empty() && options.create_program {
        programs_to_create.push(format!("asimov-{}-emitter", options.name));
    }

    if options.target_dir.exists() {
        #[cfg(feature = "tracing")]
        tracing::error!(target = ?options.target_dir, "target directory already exists");
        return Err(NewModuleError::TargetExists(options.target_dir));
    }

    let target_parent = options
        .target_dir
        .parent()
        .ok_or_else(|| NewModuleError::MissingTargetParent(options.target_dir.clone()))?;
    let target_name = target_dir_name(&options.target_dir)?;

    fs::create_dir_all(target_parent)
        .map_err(|err| NewModuleError::CreateTargetParent(target_parent.into(), err))?;

    let crate_name = options
        .package_name
        .clone()
        .unwrap_or_else(|| format!("asimov-{}-module", options.name));
    let module_label = options
        .module_label
        .clone()
        .unwrap_or_else(|| title_case_slug(&options.name));
    let module_title = options
        .module_title
        .clone()
        .unwrap_or_else(|| format!("ASIMOV {module_label} Module"));
    let package_authors = if options.package_authors.is_empty() {
        "ASIMOV Community".into()
    } else {
        options.package_authors.join(", ")
    };
    let repository_url = options
        .repository_url
        .clone()
        .unwrap_or_else(|| format!("https://github.com/asimov-modules/{crate_name}"));
    let asimov_version = options
        .asimov_version
        .clone()
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into());

    // Resolve the template to a single local checkout up front: a git
    // source is cloned exactly once here, and that same local checkout is
    // reused below both for the module's own generation and for every
    // `add_program` call — instead of each of those independently
    // re-fetching the same template.
    let (template_dir, _template_clone) = resolve_template(&options)?;

    let mut args = GenerateArgs {
        template_path: TemplatePath {
            path: Some(template_dir.to_string_lossy().into_owned()),
            ..TemplatePath::default()
        },
        name: Some(target_name.to_string_lossy().into_owned()),
        force: true,
        silent: true,
        destination: Some(target_parent.into()),
        vcs: options.vcs.as_deref().map(parse_vcs).transpose()?,
        no_workspace: true,
        ..GenerateArgs::default()
    };

    args.define = [
        Some(("package_name", crate_name.as_str())),
        options
            .package_description
            .as_deref()
            .map(|v| ("package_description", v)),
        Some(("package_authors", package_authors.as_str())),
        Some(("module_name", options.name.as_str())),
        Some(("module_label", module_label.as_str())),
        Some(("module_title", module_title.as_str())),
        options
            .module_summary
            .as_deref()
            .map(|v| ("module_summary", v)),
        Some(("repository_url", repository_url.as_str())),
        Some(("asimov_version", asimov_version.as_str())),
        Some(("publish", if options.publish { "true" } else { "false" })),
    ]
    .into_iter()
    .flatten()
    .map(|(key, value)| format!("{key}={value}"))
    .collect();

    generate(args).map_err(|err| {
        #[cfg(feature = "tracing")]
        tracing::error!(error = %err, "cargo-generate failed");
        NewModuleError::CargoGenerate(err)
    })?;

    // The template itself never scaffolds any program (its `Cargo.toml`
    // and `.asimov/module.yaml` are always bare) — every program, whether
    // it's the implicit default or an explicit list, is added uniformly
    // through `add_program`, reusing the same resolved template checkout.
    let mut program_names = Vec::new();
    for program_name in programs_to_create {
        program::add_program(program::AddProgramOptions {
            module_dir: options.target_dir.clone(),
            program_name: program_name.clone(),
            required_features: Vec::new(),
            template_path: template_dir.clone(),
        })
        .map_err(|err| NewModuleError::AddProgram(program_name.clone(), Box::new(err)))?;
        program_names.push(program_name);
    }

    #[cfg(feature = "tracing")]
    tracing::info!(
        module = %options.name,
        crate_name = %crate_name,
        target = ?options.target_dir,
        "module generated"
    );

    Ok(CreatedModule {
        module_name: options.name,
        crate_name,
        program_names,
        target_dir: options.target_dir,
    })
}

/// Resolves the template to a single local directory. A [`TemplateSource::Path`]
/// is used as-is; a [`TemplateSource::Git`] is cloned once into a temporary
/// directory, returned alongside its [`tempfile::TempDir`] guard so callers
/// can keep it alive for as long as the checkout is still needed.
///
/// This is a plain, unauthenticated clone — sufficient for the default,
/// public `asimov-template-module` repository, but not for private templates
/// requiring credentials.
fn resolve_template(
    options: &NewModuleOptions,
) -> Result<(PathBuf, Option<tempfile::TempDir>), NewModuleError> {
    match &options.template {
        TemplateSource::Path(path) => Ok((path.clone(), None)),
        TemplateSource::Git(url) => {
            let scratch = tempfile::tempdir().map_err(NewModuleError::TempDir)?;
            let mut builder = RepoBuilder::new();
            if let Some(branch) = &options.branch {
                builder.branch(branch);
            }
            builder
                .clone(url, scratch.path())
                .map_err(|err| NewModuleError::CloneTemplate(url.clone(), err))?;
            let path = scratch.path().to_path_buf();
            Ok((path, Some(scratch)))
        },
    }
}

fn parse_vcs(vcs: &str) -> Result<Vcs, NewModuleError> {
    vcs.parse().map_err(NewModuleError::CargoGenerate)
}

fn validate_module_name(name: &str) -> Result<(), NewModuleError> {
    if name.is_empty() {
        return Err(NewModuleError::EmptyName);
    }

    asimov_core::ModuleName::try_from(name)
        .map(|_| ())
        .map_err(|_| NewModuleError::InvalidName(name.into()))
}

fn validate_program_name(name: &str) -> Result<(), InvalidProgramName> {
    let Some(rest) = name.strip_prefix("asimov-") else {
        return Err(InvalidProgramName(name.into()));
    };

    if rest.is_empty() || rest.starts_with('-') || rest.ends_with('-') || !rest.contains('-') {
        return Err(InvalidProgramName(name.into()));
    }

    let valid = rest
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if valid {
        Ok(())
    } else {
        Err(InvalidProgramName(name.into()))
    }
}

/// Derives a program's "kind" from its name — the last hyphen segment.
/// Never validated against any fixed list; any word is accepted.
pub(crate) fn program_kind_of(program_name: &str) -> &str {
    program_name.rsplit('-').next().unwrap_or(program_name)
}

fn target_dir_name(target_dir: &Path) -> Result<OsString, NewModuleError> {
    target_dir
        .file_name()
        .map(OsString::from)
        .ok_or_else(|| NewModuleError::MissingTargetName(target_dir.into()))
}

fn title_case_slug(slug: &str) -> String {
    slug.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut word = first.to_uppercase().collect::<String>();
                    word.push_str(chars.as_str());
                    word
                },
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use tempfile::tempdir;

    #[test]
    fn creation_and_linting_share_module_name_boundaries() {
        let workspace = tempdir().unwrap();
        let target = workspace.path();
        fs::write(target.join("Cargo.toml"), "[package]\nname = 'fixture'\n").unwrap();
        fs::create_dir(target.join(".asimov")).unwrap();

        for (name, valid) in [
            ("x", true),
            ("a1-b2", true),
            (&"a".repeat(64), true),
            (&"a".repeat(65), false),
            ("", false),
            ("0-leading-digit", false),
            ("-sample", false),
            ("sample-", false),
            ("Sample", false),
            ("sample_name", false),
            ("sample name", false),
            ("é", false),
            ("../sample", false),
        ] {
            assert_eq!(asimov_core::ModuleName::try_from(name).is_ok(), valid);
            let result = new_module(NewModuleOptions::new(target, name));
            if valid {
                // An existing target stops generation after name validation.
                assert!(
                    matches!(result, Err(NewModuleError::TargetExists(_))),
                    "{name}"
                );
            } else {
                assert!(
                    matches!(
                        result,
                        Err(NewModuleError::EmptyName | NewModuleError::InvalidName(_))
                    ),
                    "{name}"
                );
            }

            #[cfg(feature = "lint")]
            {
                fs::write(
                    target.join(".asimov/module.yaml"),
                    format!("name: '{name}'\n"),
                )
                .unwrap();
                let findings = lint::lint_module(lint::LintOptions::new(target)).unwrap();
                assert_eq!(
                    findings
                        .iter()
                        .any(|finding| finding.code == lint::LintCode::InvalidModuleName),
                    !valid,
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn rejects_invalid_module_names() {
        assert!(matches!(
            new_module(NewModuleOptions::new("target", "")),
            Err(NewModuleError::EmptyName)
        ));
        assert!(matches!(
            new_module(NewModuleOptions::new("target", "Not Valid")),
            Err(NewModuleError::InvalidName(_))
        ));
    }

    #[test]
    fn rejects_invalid_program_name() {
        let mut options = NewModuleOptions::new("target", "widget");
        options.program_name = Some("Not-Valid".into());
        assert!(matches!(
            new_module(options),
            Err(NewModuleError::InvalidProgramName(_))
        ));
    }

    #[test]
    fn rejects_program_name_not_matching_module() {
        let mut options = NewModuleOptions::new("target", "widget");
        options.program_name = Some("asimov-other-fetcher".into());
        assert!(matches!(
            new_module(options),
            Err(NewModuleError::InvalidProgramName(_))
        ));
    }

    #[test]
    fn validates_program_name_shape() {
        assert!(validate_program_name("asimov-widget-emitter").is_ok());
        assert!(validate_program_name("asimov-widget-multi-word-kind").is_ok());
        // The kind itself is never restricted to a known list:
        assert!(validate_program_name("asimov-widget-whatever-custom-kind").is_ok());

        assert!(validate_program_name("").is_err());
        assert!(validate_program_name("widget-emitter").is_err());
        assert!(validate_program_name("asimov-widget").is_err());
        assert!(validate_program_name("asimov-Widget-Emitter").is_err());
        assert!(validate_program_name("asimov-widget-emitter-").is_err());
        assert!(validate_program_name("asimov-widgetemitter").is_err());
    }

    fn fixture_program_template(dir: &std::path::Path) {
        fs::create_dir_all(dir.join(".template")).unwrap();
        fs::write(
            dir.join(".template/program.rs.liquid"),
            "// {{ program_name }} ({{ program_kind }})\nfn main() {}\n",
        )
        .unwrap();
    }

    #[test]
    fn generates_module_from_local_template() {
        let template_dir = tempdir().unwrap();
        fs::write(
            template_dir.path().join("Cargo.toml"),
            "[package]\nname = \"{{package_name}}\"\ndescription = \"{{package_description}}\"\n",
        )
        .unwrap();
        fs::create_dir(template_dir.path().join("src")).unwrap();
        fs::write(
            template_dir.path().join("src/lib.rs"),
            "// {{module_title}}\n",
        )
        .unwrap();
        fixture_program_template(template_dir.path());

        let workspace = tempdir().unwrap();
        let target_dir = workspace.path().join("widget-module");

        let mut options =
            NewModuleOptions::new(&target_dir, "widget").template_path(template_dir.path());
        options.package_description = Some("A widget module.".into());
        let created = new_module(options).unwrap();

        assert_eq!(created.crate_name, "asimov-widget-module");
        assert_eq!(created.program_names, ["asimov-widget-emitter"]);
        assert_eq!(created.target_dir, target_dir);

        let cargo_toml = fs::read_to_string(target_dir.join("Cargo.toml")).unwrap();
        assert!(cargo_toml.contains("name = \"asimov-widget-module\""));
        assert!(cargo_toml.contains("description = \"A widget module.\""));
        assert!(cargo_toml.contains("name = \"asimov-widget-emitter\""));
        assert!(cargo_toml.contains("path = \"src/emitter/main.rs\""));

        let lib_rs = fs::read_to_string(target_dir.join("src/lib.rs")).unwrap();
        assert!(lib_rs.contains("ASIMOV Widget Module"));

        let program_rs = fs::read_to_string(target_dir.join("src/emitter/main.rs")).unwrap();
        assert!(program_rs.contains("asimov-widget-emitter"));
    }

    #[test]
    fn refuses_to_overwrite_existing_target() {
        let workspace = tempdir().unwrap();
        let target_dir = workspace.path().join("existing");
        fs::create_dir(&target_dir).unwrap();

        let options = NewModuleOptions::new(&target_dir, "widget");
        assert!(matches!(
            new_module(options),
            Err(NewModuleError::TargetExists(_))
        ));
    }

    #[test]
    fn passes_program_kind_for_a_custom_program_name() {
        let template_dir = tempdir().unwrap();
        fs::write(
            template_dir.path().join("Cargo.toml"),
            "[package]\nname = \"{{package_name}}\"\n",
        )
        .unwrap();
        fs::create_dir(template_dir.path().join("src")).unwrap();
        fs::write(template_dir.path().join("src/lib.rs"), "// lib\n").unwrap();
        fixture_program_template(template_dir.path());

        let workspace = tempdir().unwrap();
        let target_dir = workspace.path().join("widget-module");

        let mut options =
            NewModuleOptions::new(&target_dir, "widget").template_path(template_dir.path());
        options.program_name = Some("asimov-widget-fetcher".into());
        new_module(options).unwrap();

        let program_rs = fs::read_to_string(target_dir.join("src/fetcher/main.rs")).unwrap();
        assert!(program_rs.contains("(fetcher)"));
    }

    #[test]
    fn passes_full_program_kind_for_a_multi_hyphen_kind() {
        let template_dir = tempdir().unwrap();
        fs::write(
            template_dir.path().join("Cargo.toml"),
            "[package]\nname = \"{{package_name}}\"\n",
        )
        .unwrap();
        fs::create_dir(template_dir.path().join("src")).unwrap();
        fs::write(template_dir.path().join("src/lib.rs"), "// lib\n").unwrap();
        fixture_program_template(template_dir.path());

        let workspace = tempdir().unwrap();
        let target_dir = workspace.path().join("widget-module");

        let mut options =
            NewModuleOptions::new(&target_dir, "widget").template_path(template_dir.path());
        // The naive last-hyphen-segment heuristic would derive just "kind";
        // knowing the module name lets us derive the full custom kind.
        options.program_name = Some("asimov-widget-custom-multi-word-kind".into());
        new_module(options).unwrap();

        let program_rs =
            fs::read_to_string(target_dir.join("src/custom-multi-word-kind/main.rs")).unwrap();
        assert!(program_rs.contains("(custom-multi-word-kind)"));
    }

    #[test]
    fn creates_module_without_a_program() {
        let template_dir = tempdir().unwrap();
        fs::write(
            template_dir.path().join("Cargo.toml"),
            "[package]\nname = \"{{package_name}}\"\n",
        )
        .unwrap();
        fs::create_dir(template_dir.path().join("src")).unwrap();
        fs::write(template_dir.path().join("src/lib.rs"), "// lib\n").unwrap();
        fixture_program_template(template_dir.path());

        let workspace = tempdir().unwrap();
        let target_dir = workspace.path().join("bare-module");

        let options = NewModuleOptions::new(&target_dir, "bare")
            .template_path(template_dir.path())
            .without_program();
        let created = new_module(options).unwrap();

        assert!(created.program_names.is_empty());
        assert!(!target_dir.join("src/emitter").exists());
        let cargo_toml = fs::read_to_string(target_dir.join("Cargo.toml")).unwrap();
        assert!(!cargo_toml.contains("[[bin]]"));
    }

    #[test]
    fn creates_module_with_multiple_programs() {
        let template_dir = tempdir().unwrap();
        fs::write(
            template_dir.path().join("Cargo.toml"),
            "[package]\nname = \"{{package_name}}\"\n",
        )
        .unwrap();
        fs::create_dir(template_dir.path().join("src")).unwrap();
        fs::write(template_dir.path().join("src/lib.rs"), "// lib\n").unwrap();
        fixture_program_template(template_dir.path());

        let workspace = tempdir().unwrap();
        let target_dir = workspace.path().join("widget-module");

        let options = NewModuleOptions::new(&target_dir, "widget")
            .template_path(template_dir.path())
            .programs([
                "asimov-widget-emitter".to_string(),
                "asimov-widget-fetcher".to_string(),
                "asimov-widget-importer".to_string(),
            ]);
        let created = new_module(options).unwrap();

        assert_eq!(
            created.program_names,
            [
                "asimov-widget-emitter",
                "asimov-widget-fetcher",
                "asimov-widget-importer"
            ]
        );

        let cargo_toml = fs::read_to_string(target_dir.join("Cargo.toml")).unwrap();
        assert_eq!(cargo_toml.matches("[[bin]]").count(), 3);
        assert!(cargo_toml.contains("name = \"asimov-widget-fetcher\""));
        assert!(cargo_toml.contains("name = \"asimov-widget-importer\""));
        assert!(target_dir.join("src/emitter/main.rs").exists());
        assert!(target_dir.join("src/fetcher/main.rs").exists());
        assert!(target_dir.join("src/importer/main.rs").exists());
    }

    #[test]
    fn deduplicates_programs_to_create() {
        let template_dir = tempdir().unwrap();
        fs::write(
            template_dir.path().join("Cargo.toml"),
            "[package]\nname = \"{{package_name}}\"\n",
        )
        .unwrap();
        fs::create_dir(template_dir.path().join("src")).unwrap();
        fs::write(template_dir.path().join("src/lib.rs"), "// lib\n").unwrap();
        fixture_program_template(template_dir.path());

        let workspace = tempdir().unwrap();
        let target_dir = workspace.path().join("widget-module");

        // `program_name` and `extra_programs` overlapping is possible since
        // the fields are public; it shouldn't reach `add_program` twice.
        let mut options =
            NewModuleOptions::new(&target_dir, "widget").template_path(template_dir.path());
        options.program_name = Some("asimov-widget-fetcher".into());
        options.extra_programs = [
            "asimov-widget-fetcher".to_string(),
            "asimov-widget-importer".to_string(),
        ]
        .into();
        let created = new_module(options).unwrap();

        assert_eq!(
            created.program_names,
            ["asimov-widget-fetcher", "asimov-widget-importer"]
        );
    }

    #[test]
    fn rejects_extra_program_not_matching_module() {
        let template_dir = tempdir().unwrap();
        fs::write(
            template_dir.path().join("Cargo.toml"),
            "[package]\nname = \"{{package_name}}\"\n",
        )
        .unwrap();
        fs::create_dir(template_dir.path().join("src")).unwrap();
        fs::write(template_dir.path().join("src/lib.rs"), "// lib\n").unwrap();

        let workspace = tempdir().unwrap();
        let target_dir = workspace.path().join("widget-module");

        let mut options = NewModuleOptions::new(&target_dir, "widget")
            .template_path(template_dir.path())
            .without_program();
        options.extra_programs = ["asimov-other-fetcher".to_string()].into();

        assert!(matches!(
            new_module(options),
            Err(NewModuleError::InvalidProgramName(_))
        ));
        assert!(!target_dir.exists());
    }

    #[test]
    #[ignore = "network smoke test against the real default template; run manually"]
    fn smoke_test_default_git_template_with_multiple_programs() {
        let workspace = tempdir().unwrap();
        let target_dir = workspace.path().join("widget-module");

        // A `Git` source is cloned exactly once here, and that single
        // checkout is reused for both the base module and every extra
        // program below — not re-fetched per program.
        let options = NewModuleOptions::new(&target_dir, "widget").programs([
            "asimov-widget-fetcher".to_string(),
            "asimov-widget-importer".to_string(),
        ]);
        let created = new_module(options).unwrap();

        assert_eq!(
            created.program_names,
            ["asimov-widget-fetcher", "asimov-widget-importer"]
        );
        assert!(target_dir.join("src/fetcher/main.rs").exists());
        assert!(target_dir.join("src/importer/main.rs").exists());
    }
}
