// This is free and unencumbered software released into the public domain.

//! Shared execution timing options.

#[cfg(any(feature = "std", feature = "clap"))]
use alloc::{format, string::String};
use bon::Builder;
use core::time::Duration;

#[cfg(feature = "clap")]
const HELP_DEADLINE: &str = r#"Allow at most this duration for the command.

Use a relative duration such as 30s or "1m 30s", not an absolute timestamp.
Zero is accepted. If omitted, timing policy is command-specific."#;

/// Execution timing requests shared by program commands.
///
/// Flatten this alongside a program's options in a host's Clap arguments.
/// The deadline is a relative duration, not an absolute timestamp. It defaults
/// to `None`, leaving timing policy to the implementation. Clap accepts
/// human-readable durations such as `30s` or `1m 30s`, including zero.
///
/// These options do not enforce timeouts. Hosts can forward an explicit value
/// using `deadline_option()` (available with `std` or `clap`). Cache freshness
/// is configured separately by [`crate::CachingOptions`].
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug))]
#[cfg_attr(feature = "clap", derive(clap::Args))]
#[cfg_attr(feature = "clap", command(about = None, long_about = None))]
pub struct TimingOptions {
    /// Allow at most this duration for the command (for example, 30s).
    #[cfg_attr(
        feature = "clap",
        clap(
            long,
            value_name = "DURATION",
            value_parser = humantime::parse_duration,
            help = "Allow at most this duration for the command",
            long_help = HELP_DEADLINE
        )
    )]
    pub deadline: Option<Duration>,
}

#[cfg(any(feature = "std", feature = "clap"))]
impl TimingOptions {
    /// Formats an explicit deadline as one `--deadline=DURATION` argument.
    ///
    /// Returns `None` when unset; a zero duration is forwarded as `0s`.
    pub fn deadline_option(&self) -> Option<String> {
        self.deadline
            .map(|value| format!("--deadline={}", humantime::format_duration(value)))
    }
}
