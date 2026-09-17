// This is free and unencumbered software released into the public domain.

use alloc::string::{String, ToString};
use derive_more::Display;
use thiserror::Error;

/// An error encountered when parsing a [`SocialProperty`] selector.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ParseSocialPropertyError {
    /// The input does not match a supported, case-sensitive property name.
    #[error("unknown social property: {0}")]
    UnknownProperty(
        /// The original input, preserved without trimming or normalization.
        String,
    ),
}

/// A selector for an account's identity, connections, or content.
///
/// This enum identifies a property; it does not contain the property's value.
/// Parsing accepts the lowercase names `url`, `id`, `handle`, `name`,
/// `followers`, `followees`, `mutuals`, and `posts`. The alias `following` also
/// selects [`Followees`](Self::Followees). Input is case-sensitive and is not
/// trimmed. Display formatting uses the Rust variant name (for example, `Url`),
/// rather than the lowercase parsing spelling.
///
/// # Examples
///
/// ```
/// use asimov_social::SocialProperty;
///
/// assert_eq!("following".parse::<SocialProperty>()?, SocialProperty::Followees);
/// assert!("Followers".parse::<SocialProperty>().is_err());
/// # Ok::<(), asimov_social::ParseSocialPropertyError>(())
/// ```
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum SocialProperty {
    /// The URL of the account's profile.
    Url,
    /// The account's platform-specific identifier.
    Id,
    /// The account's handle or username on the platform.
    Handle,
    /// The account's display name.
    Name,
    /// Accounts that follow the selected account.
    Followers,
    /// Accounts followed by the selected account; also parsed as `following`.
    Followees,
    /// Accounts that both follow and are followed by the selected account.
    Mutuals,
    /// Posts associated with the selected account.
    Posts,
}

impl core::str::FromStr for SocialProperty {
    type Err = ParseSocialPropertyError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        use SocialProperty::*;
        Ok(match input {
            "url" => Url,
            "id" => Id,
            "handle" => Handle,
            "name" => Name,
            "followers" => Followers,
            "followees" | "following" => Followees,
            "mutuals" => Mutuals,
            "posts" => Posts,
            _ => return Err(ParseSocialPropertyError::UnknownProperty(input.to_string())),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::SocialProperty;
    use core::str::FromStr;

    #[test]
    fn parses_handle_social_property() {
        assert_eq!(
            SocialProperty::from_str("handle"),
            Ok(SocialProperty::Handle)
        );
    }
}
