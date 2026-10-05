// This is free and unencumbered software released into the public domain.

use alloc::{collections::BTreeMap, string::String, vec::Vec};

/// See: <https://asimov-specs.github.io/module-manifest/>
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct ModuleManifest {
    /// See: <https://asimov-specs.github.io/module-manifest/#name-field>
    pub name: String,

    /// See: <https://asimov-specs.github.io/module-manifest/#label-field>
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub label: Option<String>,

    /// See: <https://asimov-specs.github.io/module-manifest/#title-field>
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub title: Option<String>,

    /// See: <https://asimov-specs.github.io/module-manifest/#summary-field>
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub summary: Option<String>,

    /// See: <https://asimov-specs.github.io/module-manifest/#links-field>
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub links: Vec<String>,

    /// See: <https://asimov-specs.github.io/module-manifest/#tags-field>
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub tags: Vec<String>,

    /// See: <https://asimov-specs.github.io/module-manifest/#requires-section>
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Requires::is_empty")
    )]
    pub requires: Requires,

    /// See: <https://asimov-specs.github.io/module-manifest/#provides-section>
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Provides::is_empty")
    )]
    pub provides: Provides,

    /// See: <https://asimov-specs.github.io/module-manifest/#handles-section>
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Handles::is_empty")
    )]
    pub handles: Handles,

    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            alias = "configuration",
            skip_serializing_if = "Option::is_none"
        )
    )]
    pub config: Option<Configuration>,
}

#[cfg(feature = "std")]
#[derive(Debug, thiserror::Error)]
pub enum ReadVarError {
    /// A profile, module, or variable name is not a single filename component.
    #[error("invalid {component}: {source}")]
    InvalidComponent {
        component: &'static str,
        #[source]
        source: asimov_core::InvalidFilenameComponent,
    },

    #[error("variable named `{0}` not found in module manifest")]
    UnknownVar(String),

    #[error("a value for variable `{0}` was not configured")]
    UnconfiguredVar(String),

    #[error("failed to read variable `{name}`: {source}")]
    Io {
        name: String,
        #[source]
        source: std::io::Error,
    },
}

impl ModuleManifest {
    /// Reads a manifest using a single portable module-name component.
    ///
    /// Invalid names return `InvalidInput`. Relative symlinks may resolve only
    /// within the directory being searched; escaping links return an I/O error.
    #[cfg(all(feature = "std", feature = "serde"))]
    pub fn read_manifest(module_name: &str) -> std::io::Result<Self> {
        Self::read_manifest_with_root(module_name, asimov_env::paths::asimov_root)
    }

    #[cfg(all(feature = "std", feature = "serde"))]
    fn read_manifest_with_root(
        module_name: &str,
        root: impl FnOnce() -> std::path::PathBuf,
    ) -> std::io::Result<Self> {
        use asimov_core::{crates::cap_std, validate_filename_component};
        validate_filename_component(module_name)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
        let root = cap_std::fs::Dir::open_ambient_dir(root(), cap_std::ambient_authority())?;
        let directory = root.open_dir("modules")?;
        let search_paths = [
            ("installed", "json"),
            ("installed", "yaml"), // legacy, new installs are converted to JSON
            ("", "yaml"),          // legacy, new installs go to `installed/`
        ];

        for (sub_dir, ext) in search_paths {
            let file = alloc::format!("{module_name}.{ext}");
            let content = if sub_dir.is_empty() {
                directory.read(&file)
            } else {
                directory.open_dir(sub_dir).and_then(|dir| dir.read(&file))
            };
            match content {
                Ok(content) if ext == "json" => {
                    return serde_json::from_slice(&content).map_err(std::io::Error::other);
                },
                Ok(content) if ext == "yaml" => {
                    return serde_yaml_ng::from_slice(&content).map_err(std::io::Error::other);
                },
                Ok(_) => unreachable!(),

                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => return Err(err),
            }
        }

        Err(std::io::ErrorKind::NotFound.into())
    }

    #[cfg(feature = "std")]
    pub fn read_variables(
        &self,
        profile: Option<&str>,
    ) -> Result<alloc::collections::BTreeMap<String, String>, ReadVarError> {
        Self::validate_component("module name", &self.name)?;
        Self::validate_component("profile name", profile.unwrap_or("default"))?;
        self.config
            .as_ref()
            .map(|c| c.variables.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|var| Ok((var.name.clone(), self.variable(&var.name, profile)?)))
            .collect()
    }

    /// Reads a configuration value after validating all filename components.
    ///
    /// Invalid names are rejected even for environment/default values. File
    /// access is confined to the selected profile and module directories;
    /// symlinks cannot escape them. I/O errors other than absence propagate.
    #[cfg(feature = "std")]
    pub fn variable(&self, key: &str, profile: Option<&str>) -> Result<String, ReadVarError> {
        self.variable_with_root(key, profile, asimov_env::paths::asimov_root)
    }

    #[cfg(feature = "std")]
    fn validate_component(component: &'static str, value: &str) -> Result<(), ReadVarError> {
        asimov_core::validate_filename_component(value)
            .map_err(|source| ReadVarError::InvalidComponent { component, source })
    }

    #[cfg(feature = "std")]
    fn variable_with_root(
        &self,
        key: &str,
        profile: Option<&str>,
        root: impl FnOnce() -> std::path::PathBuf,
    ) -> Result<String, ReadVarError> {
        use asimov_core::crates::cap_std;
        let profile = profile.unwrap_or("default");
        Self::validate_component("module name", &self.name)?;
        Self::validate_component("profile name", profile)?;
        Self::validate_component("variable name", key)?;
        let Some(var) = self
            .config
            .as_ref()
            .and_then(|conf| conf.variables.iter().find(|var| var.name == key))
        else {
            return Err(ReadVarError::UnknownVar(key.into()));
        };

        if let Some(value) = var
            .environment
            .as_deref()
            .and_then(|env_name| std::env::var(env_name).ok())
        {
            return Ok(value);
        }

        let read = || -> std::io::Result<String> {
            let root = cap_std::fs::Dir::open_ambient_dir(root(), cap_std::ambient_authority())?;
            root.open_dir("configs")?
                .open_dir(profile)?
                .open_dir(&self.name)?
                .read_to_string(key)
        };
        read().or_else(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                var.default_value
                    .clone()
                    .ok_or_else(|| ReadVarError::UnconfiguredVar(key.into()))
            } else {
                Err(ReadVarError::Io {
                    name: key.into(),
                    source: err,
                })
            }
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Requires {
    /// The set of modules that this module depends on.
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub modules: Vec<String>,

    /// The set of platforms that this module depends on.
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub platforms: Vec<String>,

    /// The set of programs that this module depends on.
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub programs: Vec<String>,

    /// The set of libraries that this module depends on.
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub libraries: Vec<String>,

    /// The set of models that this module depends on.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "BTreeMap::is_empty")
    )]
    pub models: BTreeMap<String, RequiredModel>,

    /// The set of datasets that this module depends on.
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub datasets: Vec<String>,

    /// The set of ontologies that this module depends on.
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub ontologies: Vec<String>,

    /// The set of classes that this module depends on.
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub classes: Vec<String>,

    /// The set of datatypes that this module depends on.
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub datatypes: Vec<String>,
}

impl Requires {
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
            && self.platforms.is_empty()
            && self.programs.is_empty()
            && self.libraries.is_empty()
            && self.models.is_empty()
            && self.datasets.is_empty()
            && self.ontologies.is_empty()
            && self.classes.is_empty()
            && self.datatypes.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(untagged)
)]
pub enum RequiredModel {
    /// Just a direct URL string:
    /// ```yaml
    /// hf:first/model: model_file.bin
    /// ```
    Url(String),

    /// Multiple variants:
    /// ```yaml
    /// hf:second/model:
    ///   small: model_small.bin
    ///   medium: model_medium.bin
    ///   large: model_large.bin
    /// ```
    #[cfg_attr(
        feature = "serde",
        serde(
            deserialize_with = "ordered::deserialize_ordered",
            serialize_with = "ordered::serialize_ordered"
        )
    )]
    Choices(Vec<(String, String)>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Provides {
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub programs: Vec<String>,
}

impl Provides {
    pub fn is_empty(&self) -> bool {
        self.programs.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Handles {
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub url_protocols: Vec<String>,

    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub url_prefixes: Vec<String>,

    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub url_patterns: Vec<String>,

    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub file_extensions: Vec<String>,

    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            deserialize_with = "empty_vec_if_null",
            skip_serializing_if = "Vec::is_empty"
        )
    )]
    pub content_types: Vec<String>,
}

impl Handles {
    pub fn is_empty(&self) -> bool {
        self.url_protocols.is_empty()
            && self.url_prefixes.is_empty()
            && self.url_patterns.is_empty()
            && self.file_extensions.is_empty()
            && self.content_types.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Configuration {
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub variables: Vec<ConfigurationVariable>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct ConfigurationVariable {
    /// The name of the variable. Configured variables are by default saved in
    /// `~/.asimov/configs/$profile/$module/$name`.
    pub name: String,

    /// Optional description to provide information about the variable.
    #[cfg_attr(
        feature = "serde",
        serde(default, alias = "desc", skip_serializing_if = "Option::is_none")
    )]
    pub description: Option<String>,

    /// Optional name of an environment variable to check for a value before checking for a
    /// configured or a default value.
    #[cfg_attr(
        feature = "serde",
        serde(default, alias = "env", skip_serializing_if = "Option::is_none")
    )]
    pub environment: Option<String>,

    /// Optional default value to use as a fallback. If a default value is present the user
    /// configuration of the value is not required.
    #[cfg_attr(
        feature = "serde",
        serde(default, alias = "default", skip_serializing_if = "Option::is_none")
    )]
    pub default_value: Option<String>,

    /// Whether the value is sensitive, such as a credential or an access token.
    /// Tools must not display secret values unless explicitly asked to.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "core::ops::Not::not")
    )]
    pub secret: bool,

    /// Whether the module operates without a value for this variable. Only
    /// meaningful for variables that have no default value, which are otherwise
    /// taken to be required; see [`ConfigurationVariable::is_required`].
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "core::ops::Not::not")
    )]
    pub optional: bool,
}

impl ConfigurationVariable {
    /// Whether a value must be configured for the module to operate.
    pub fn is_required(&self) -> bool {
        !self.optional && self.default_value.is_none()
    }
}

#[cfg(feature = "serde")]
fn empty_vec_if_null<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    use serde::Deserialize;
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

#[cfg(feature = "serde")]
mod ordered {
    use super::*;
    use alloc::fmt;
    use serde::{
        Deserializer,
        de::{MapAccess, Visitor},
    };

    pub fn serialize_ordered<S>(
        items: &[(String, String)],
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_map(items.iter().map(|(key, value)| (key, value)))
    }

    pub fn deserialize_ordered<'de, D>(deserializer: D) -> Result<Vec<(String, String)>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OrderedVisitor;

        impl<'de> Visitor<'de> for OrderedVisitor {
            type Value = Vec<(String, String)>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a map of string keys to string values (preserving order)")
            }

            fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut items = Vec::with_capacity(access.size_hint().unwrap_or(0));
                while let Some((k, v)) = access.next_entry::<String, String>()? {
                    items.push((k, v));
                }
                Ok(items)
            }
        }

        deserializer.deserialize_map(OrderedVisitor)
    }
}

#[cfg(all(test, feature = "std"))]
#[path = "module_manifest_paths.rs"]
mod path_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn model_choices_round_trip_in_order() {
        let yaml = "name: example\nrequires:\n  models:\n    hf:example/model:\n      small: small.bin\n      medium: medium.bin\n      large: large.bin\n";
        let manifest: ModuleManifest = serde_yaml_ng::from_str(yaml).unwrap();
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains(
            r#""hf:example/model":{"small":"small.bin","medium":"medium.bin","large":"large.bin"}"#
        ));
        assert_eq!(
            serde_json::from_str::<ModuleManifest>(&json).unwrap(),
            manifest
        );
        let yaml = serde_yaml_ng::to_string(&manifest).unwrap();
        assert_eq!(
            serde_yaml_ng::from_str::<ModuleManifest>(&yaml).unwrap(),
            manifest
        );
    }

    #[test]
    fn dependency_fields_round_trip_independently() {
        for field in [
            "modules",
            "platforms",
            "programs",
            "libraries",
            "datasets",
            "ontologies",
            "classes",
            "datatypes",
        ] {
            let json = serde_json::json!({"name": "example", "requires": {field: ["dependency"]}});
            let manifest: ModuleManifest = serde_json::from_value(json.clone()).unwrap();
            assert!(!manifest.requires.is_empty(), "{field}");
            assert_eq!(serde_json::to_value(&manifest).unwrap(), json);
            let yaml = serde_yaml_ng::to_string(&manifest).unwrap();
            assert_eq!(
                serde_yaml_ng::from_str::<ModuleManifest>(&yaml).unwrap(),
                manifest
            );
        }
        assert!(Requires::default().is_empty());
    }

    #[test]
    fn test_deser() {
        let yaml = r#"
name: example
label: Example
summary: Example Module
links:
  - https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-module

requires:
    modules:
      - other
    models:
      hf:first/model: first_url
      hf:second/model:
        small: small_url
        medium: medium_url
        large: large_url

provides:
  programs:
    - asimov-example-module

handles:
  content_types:
    - content_type
  file_extensions:
    - file_extension
  url_patterns:
    - pattern
  url_prefixes:
    - prefix
  url_protocols:
    - protocol

config:
  variables:
    - name: api_key
      description: "api key to authorize requests"
      default_value: "foobar"
      environment: API_KEY

    - name: token
      secret: true

    - name: nickname
      optional: true

"#;

        let dec: ModuleManifest = serde_yaml_ng::from_str(yaml).expect("deser should succeed");

        assert_eq!(dec.name, "example");
        assert_eq!(dec.label.as_deref(), Some("Example"));
        assert_eq!(dec.summary.as_deref(), Some("Example Module"));

        assert_eq!(
            dec.links,
            vec!["https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-module"],
        );

        assert_eq!(dec.provides.programs.len(), 1);
        assert_eq!(
            dec.provides.programs.first().unwrap(),
            "asimov-example-module",
        );

        assert_eq!(
            dec.handles
                .content_types
                .first()
                .expect("should have content_types"),
            "content_type",
        );

        assert_eq!(
            dec.handles
                .file_extensions
                .first()
                .expect("should have file_extensions"),
            "file_extension",
        );

        assert_eq!(
            dec.handles
                .url_patterns
                .first()
                .expect("should have url_patterns"),
            "pattern",
        );

        assert_eq!(
            dec.handles
                .url_prefixes
                .first()
                .expect("should have url_prefixes"),
            "prefix",
        );

        assert_eq!(
            dec.handles
                .url_protocols
                .first()
                .expect("should have url_protocols"),
            "protocol",
        );

        let variables = dec.config.expect("should have config").variables;

        assert_eq!(
            variables.first(),
            Some(&ConfigurationVariable {
                name: "api_key".into(),
                description: Some("api key to authorize requests".into()),
                environment: Some("API_KEY".into()),
                default_value: Some("foobar".into()),
                secret: false,
                optional: false,
            }),
        );
        assert!(!variables[0].is_required());

        assert!(variables[1].secret);
        assert!(variables[1].is_required());

        assert!(variables[2].optional);
        assert!(!variables[2].is_required());

        let requires = dec.requires;

        assert_eq!(requires.modules.len(), 1);
        assert_eq!(requires.modules.first().unwrap(), "other");

        assert_eq!(requires.models.len(), 2);

        assert_eq!(
            requires.models["hf:first/model"],
            RequiredModel::Url("first_url".into()),
        );

        assert_eq!(
            requires.models["hf:second/model"],
            RequiredModel::Choices(vec![
                ("small".into(), "small_url".into()),
                ("medium".into(), "medium_url".into()),
                ("large".into(), "large_url".into())
            ]),
        );
    }
}
