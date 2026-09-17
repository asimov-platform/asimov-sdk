// This is free and unencumbered software released into the public domain.

//! Account identifiers for platforms without a `known-types` handle wrapper.
//!
//! Parsers accept percent-encoded UTF-8 and retain decoded identifiers.

use alloc::string::{String, ToString};
use core::str::FromStr;
use derive_more::Display;
use known_types::handle::ParseHandleError;

// Keep identifiers private so conversions cannot bypass the parser.
macro_rules! handle_type {
    ($feature:literal, $name:ident, $doc:literal, $validate:expr) => {
        #[doc = $doc]
        #[cfg(feature = $feature)]
        #[derive(Clone, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
        #[display("{_0}")]
        pub struct $name(String);

        #[cfg(feature = $feature)]
        impl $name {
            /// Borrows the identifier without a URL prefix or display-name `@`.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        #[cfg(feature = $feature)]
        impl FromStr for $name {
            type Err = ParseHandleError;

            fn from_str(input: &str) -> Result<Self, Self::Err> {
                let input = percent_encoding::percent_decode_str(input)
                    .decode_utf8()
                    .map_err(|_| ParseHandleError::InvalidUtf8)?;
                let input = input.as_ref();
                if !($validate)(input) {
                    return Err(ParseHandleError::InvalidFormat);
                }
                Ok(Self(input.to_string()))
            }
        }
    };
}

/// Encodes a handle for use as a single URL path segment.
#[cfg(feature = "youtube")]
pub(crate) fn encoded_handle(input: &str) -> impl core::fmt::Display + '_ {
    percent_encoding::utf8_percent_encode(input, percent_encoding::CONTROLS)
}

#[cfg(any(
    feature = "bluesky",
    feature = "gitlab",
    feature = "medium",
    feature = "pinterest",
    feature = "reddit",
    feature = "snapchat",
    feature = "substack",
    feature = "threads",
    feature = "tiktok",
    feature = "twitch"
))]
fn ascii_name(input: &str, min: usize, max: usize, punctuation: &str) -> bool {
    (min..=max).contains(&input.len())
        && input
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || punctuation.contains(c))
}

handle_type!(
    "bluesky",
    BlueskyHandle,
    "A Bluesky domain handle: ASCII DNS labels, at most 253 bytes, with an alphabetic TLD start. DIDs are profile selectors, not handles.",
    |s: &str| s.len() <= 253
        && s.contains('.')
        && s.split('.').all(|label| ascii_name(label, 1, 63, "-")
            && !label.starts_with('-')
            && !label.ends_with('-'))
        && s.rsplit('.')
            .next()
            .unwrap_or("")
            .starts_with(|c: char| c.is_ascii_alphabetic())
);

handle_type!(
    "discord",
    DiscordHandle,
    "A Discord profile identifier: a nonzero decimal `u64` snowflake, not a username. Leading zeroes are rejected.",
    |s: &str| !s.starts_with('0')
        && s.bytes().all(|b| b.is_ascii_digit())
        && s.parse::<u64>().is_ok_and(|id| id > 0)
);

handle_type!(
    "gitlab",
    GitlabHandle,
    "A GitLab.com username: 2–255 ASCII letters, digits, `_`, `-`, or `.`; no leading punctuation, trailing dot, repeated dots, or `.git`/`.atom` suffix. Reserved names are not checked.",
    |s: &str| ascii_name(s, 2, 255, "_-.")
        && s.starts_with(|c: char| c.is_ascii_alphanumeric())
        && !s.ends_with('.')
        && !s.contains("..")
        && !s.ends_with(".git")
        && !s.ends_with(".atom")
);

handle_type!(
    "medium",
    MediumHandle,
    "A Medium profile identifier. Accepts a nonempty ASCII alphanumeric, `_`, `-`, or `.` path segment; account availability and registration rules are not checked.",
    |s: &str| ascii_name(s, 1, 255, "_-.") && s != "." && s != ".."
);

handle_type!(
    "pinterest",
    PinterestHandle,
    "A Pinterest username: 3–30 ASCII letters, digits, or underscores.",
    |s: &str| ascii_name(s, 3, 30, "_")
);

handle_type!(
    "reddit",
    RedditHandle,
    "A Reddit username: 3–20 ASCII letters, digits, underscores, or hyphens.",
    |s: &str| ascii_name(s, 3, 20, "_-")
);

handle_type!(
    "snapchat",
    SnapchatHandle,
    "A Snapchat username: 3–15 ASCII letters, digits, `_`, `-`, or `.`; starts with a letter and ends with a letter or digit.",
    |s: &str| ascii_name(s, 3, 15, "_-.")
        && s.starts_with(|c: char| c.is_ascii_alphabetic())
        && s.ends_with(|c: char| c.is_ascii_alphanumeric())
);

handle_type!(
    "substack",
    SubstackHandle,
    "A Substack profile identifier. Accepts a nonempty ASCII alphanumeric, `_`, or `-` path segment; publication names and account availability are not checked.",
    |s: &str| ascii_name(s, 1, 255, "_-")
);

handle_type!(
    "threads",
    ThreadsHandle,
    "A Threads username: 1–30 ASCII letters, digits, underscores, or dots; no leading, trailing, or consecutive dots.",
    |s: &str| ascii_name(s, 1, 30, "_.")
        && !s.starts_with('.')
        && !s.ends_with('.')
        && !s.contains("..")
);

handle_type!(
    "tiktok",
    TiktokHandle,
    "A TikTok username: 2–24 ASCII letters, digits, underscores, or dots, with no trailing dot.",
    |s: &str| ascii_name(s, 2, 24, "_.") && !s.ends_with('.')
);

handle_type!(
    "twitch",
    TwitchHandle,
    "A Twitch username: 4–25 ASCII letters, digits, or underscores.",
    |s: &str| ascii_name(s, 4, 25, "_")
);

handle_type!(
    "youtube",
    YoutubeHandle,
    "A YouTube handle without `@`. Accepts 1–30 Unicode letters/digits with internal `_`, `-`, `.`, or `·`; language-specific length and script restrictions are not checked.",
    |s: &str| (1..=30).contains(&s.chars().count())
        && s.starts_with(char::is_alphanumeric)
        && s.ends_with(char::is_alphanumeric)
        && s.chars().all(|c| c.is_alphanumeric() || "_-.·".contains(c))
);
