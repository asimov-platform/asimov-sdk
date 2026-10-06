// This is free and unencumbered software released into the public domain.

//! Shared output filtering options.

use alloc::string::String;
use bon::Builder;

#[cfg(feature = "clap")]
const HELP_JEV: &str = r#"Filter output using a Jev noul: an assertion evaluated as true or false.

Use an assertion such as "The name is Ukrainian", rather than a question.
Jev filtering is applied before --jq."#;

#[cfg(feature = "clap")]
const HELP_JQ: &str = r#"Filter and/or transform JSON-LD output using a jq expression.

For example, "select(.name)". When --jev is also specified, jq is applied after
Jev filtering."#;

/// Output filtering requests for host commands.
///
/// Flatten this alongside [`crate::FetcherOptions`] or [`crate::ListerOptions`]
/// in a host's Clap arguments. Both fields default to `None`, leaving output
/// unfiltered. The host applies Jev before jq and supplies the evaluators and any
/// required credentials. This type stores expressions without evaluating them;
/// `asimov-runner` does not perform this filtering.
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
    /// before [`Self::jq`].
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

    /// Filter and/or transform JSON-LD output using a jq expression.
    ///
    /// For example, `select(.name)`. Applied by the host after [`Self::jev`].
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
