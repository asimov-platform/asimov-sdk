// This is free and unencumbered software released into the public domain.

use alloc::string::{String, ToString};
use derive_more::Display;
use known_types_x::ParseHandleError;
use thiserror::Error;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ParseSocialPropertyError {
    #[error("unknown social property: {0}")]
    UnknownProperty(String),
}

#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum SocialProperty {
    Url,
    Id,
    Handle,
    Name,
    Followers,
    Followees,
    Mutuals,
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
