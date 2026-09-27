// This is free and unencumbered software released into the public domain.

//! Shared cache freshness options for source commands.

#[cfg(feature = "clap")]
use alloc::string::ToString;
#[cfg(any(feature = "std", feature = "clap"))]
use alloc::{format, string::String};
use bon::Builder;
use core::time::Duration;

#[cfg(feature = "clap")]
const HELP_MAX_AGE: &str = r#"Maximum acceptable cache age.

Use a positive duration such as 1h, 7d, or "1m 30s".
If omitted, the default is module-specific."#;

/// Cache freshness requests shared by fetcher and lister commands.
///
/// Flatten this alongside [`crate::FetcherOptions`] or [`crate::ListerOptions`]
/// in a host's Clap arguments. The maximum age defaults to `None`, leaving
/// policy to the selected module. Clap accepts durations such as `1h`,
/// `7d`, or `1m 30s`; it rejects a zero maximum age.
/// Builders and direct field assignments do not validate durations.
///
/// These options describe requests; they do not implement caching. Hosts can
/// forward explicit values using `max_age_option()` (available with `std` or
/// `clap`). Execution deadlines are configured separately by
/// [`crate::TimingOptions`].
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug))]
#[cfg_attr(feature = "clap", derive(clap::Args))]
#[cfg_attr(feature = "clap", command(about = None, long_about = None))]
pub struct CachingOptions {
    /// Maximum acceptable cache age (for example, 1h or 7d).
    /// Default is module-specific.
    #[cfg_attr(
        feature = "clap",
        clap(
            long,
            value_name = "DURATION",
            value_parser = parse_max_age,
            help = "Maximum acceptable cache age",
            long_help = HELP_MAX_AGE
        )
    )]
    pub max_age: Option<Duration>,
}

#[cfg(any(feature = "std", feature = "clap"))]
impl CachingOptions {
    /// Formats an explicit maximum age as one `--max-age=DURATION` argument.
    ///
    /// Returns `None` when unset, preserving the module's default policy.
    pub fn max_age_option(&self) -> Option<String> {
        self.max_age
            .map(|value| format!("--max-age={}", humantime::format_duration(value)))
    }
}

#[cfg(feature = "clap")]
fn parse_max_age(value: &str) -> Result<Duration, String> {
    let duration = humantime::parse_duration(value).map_err(|error| error.to_string())?;
    if duration.is_zero() {
        return Err("max age must be positive".into());
    }
    Ok(duration)
}
