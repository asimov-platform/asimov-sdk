// This is free and unencumbered software released into the public domain.

//! Shared output filtering options.

use alloc::{string::String, vec::Vec};
use bon::Builder;

#[cfg(feature = "clap")]
const HELP_JEV: &str = r#"Filter output using a Jev noul: an assertion evaluated as true or false.

Use an assertion such as "The name is Ukrainian", rather than a question.
Jev sees the original records, before --json property selection and --jq."#;

#[cfg(feature = "clap")]
const HELP_JSON: &str = r#"Select only the named top-level JSON properties.

Provide a nonempty, comma-separated list, such as --json=name,url. May be
repeated to select additional properties. Names are literal and case-sensitive;
missing properties are omitted. Selected values retain their nested contents.
Arrays of objects retain their shape. JSON-LD keys must be explicitly selected.

Use --json=@id,name,url for lists starting with @ to avoid argument-file
expansion.

Requires JSONL output. Applied after Jev filtering and before --jq; jq may then
rename properties or produce other values."#;

#[cfg(feature = "clap")]
const HELP_JQ: &str = r#"Filter and/or transform JSON-LD output using a jq expression.

For example, "select(.name)". Applied after Jev filtering and --json property
selection; jq may rename properties, create new objects, or produce scalars."#;

/// Output filtering requests for host commands.
///
/// Flatten this alongside [`crate::FetcherOptions`] or [`crate::ListerOptions`]
/// in a host's Clap arguments. All fields default to `None`, leaving output
/// unfiltered. The host applies Jev, then JSON property selection, then jq, and
/// supplies the evaluators and any required credentials. This type stores
/// filtering requests without evaluating them; `asimov-runner` does not perform
/// this filtering.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[builder(derive(Debug), on(String, into))]
#[cfg_attr(feature = "clap", derive(clap::Args))]
#[cfg_attr(feature = "clap", command(about = None, long_about = None))]
pub struct FilteringOptions {
    /// Filter output using a Jev noul (an assertion evaluated as true or false).
    ///
    /// The `NOUL` argument should be an assertion, such as
    /// `The name is Ukrainian`, rather than a question. Applied by the host
    /// before [`Self::json`] and [`Self::jq`], using the original records.
    #[cfg_attr(
        feature = "clap",
        clap(
            long,
            value_name = "NOUL",
            help = "Filter output using a Jev noul (an assertion evaluated as true or false)",
            long_help = HELP_JEV
        )
    )]
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub jev: Option<String>,

    /// Select only these literal, case-sensitive top-level JSON property names.
    ///
    /// Applied after [`Self::jev`] and before [`Self::jq`]. Missing properties
    /// are omitted; objects with no matches become `{}`. Selected values retain
    /// their nested contents. Arrays of objects retain their shape; other input
    /// values are errors. JSON-LD keys such as `@id` and `@context` receive no
    /// special treatment and must be explicitly selected.
    ///
    /// Requires JSONL output and at least one nonempty name. Clap validates the
    /// names and accepts repeated comma-separated lists. Hosts must validate
    /// programmatic selections. With Serde, this is an array of names; `None`
    /// omits the selection. Duplicate names have no additional effect.
    #[cfg_attr(
        feature = "clap",
        clap(
            long,
            value_name = "FIELDS",
            value_delimiter = ',',
            num_args = 1,
            value_parser = clap::builder::NonEmptyStringValueParser::new(),
            help = "Select only the named top-level JSON properties",
            long_help = HELP_JSON
        )
    )]
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub json: Option<Vec<String>>,

    /// Filter and/or transform JSON-LD output using a jq expression.
    ///
    /// For example, `select(.name)`. Applied by the host after [`Self::jev`] and
    /// [`Self::json`]. May produce properties or values outside the selection.
    #[cfg_attr(
        feature = "clap",
        clap(
            long,
            value_name = "EXPR",
            help = "Filter and/or transform JSON-LD output using a jq expression",
            long_help = HELP_JQ
        )
    )]
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub jq: Option<String>,
}
