// This is free and unencumbered software released into the public domain.

use crate::IdError;
use core::str::FromStr;
use derive_more::Display;

#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde_with::SerializeDisplay, serde_with::DeserializeFromStr)
)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum IdClass {
    #[display("B")]
    Blob,
    #[display("E")]
    Event,
    #[display("O")]
    Organization,
    #[display("P")]
    Person,
}

impl IdClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Blob => "B",
            Self::Event => "E",
            Self::Organization => "O",
            Self::Person => "P",
        }
    }

    pub fn as_char(&self) -> char {
        match self {
            Self::Blob => 'B',
            Self::Event => 'E',
            Self::Organization => 'O',
            Self::Person => 'P',
        }
    }

    #[cfg(feature = "std")]
    pub fn yaml_path(&self) -> std::path::PathBuf {
        match self {
            Self::Blob => "blob.yaml",
            Self::Event => "event.yaml",
            Self::Organization => "organization.yaml",
            Self::Person => "person.yaml",
        }
        .into()
    }

    #[cfg(feature = "std")]
    pub fn dir_path(&self) -> std::path::PathBuf {
        match self {
            Self::Blob => "blobs",
            Self::Event => "events",
            Self::Organization => "organizations",
            Self::Person => "people",
        }
        .into()
    }
}

impl FromStr for IdClass {
    type Err = IdError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Ok(match input {
            "B" => Self::Blob,
            "E" => Self::Event,
            "O" => Self::Organization,
            "P" => Self::Person,
            _ => return Err(IdError::UnknownClass),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn class_parsing_requires_exactly_one_designator() {
        for class in [
            IdClass::Blob,
            IdClass::Event,
            IdClass::Organization,
            IdClass::Person,
        ] {
            assert_eq!(class.as_str().parse::<IdClass>().unwrap(), class);
            for suffix in ["extra", " ", "\n", "é"] {
                assert!(
                    alloc::format!("{class}{suffix}")
                        .parse::<IdClass>()
                        .is_err()
                );
            }
            let id = crate::Id::<16>::zero(class);
            assert_eq!(id.to_string().parse::<crate::Id<16>>().unwrap(), id);
        }
        for invalid in ["", "p", "X", "é", "🦀"] {
            assert!(invalid.parse::<IdClass>().is_err());
            assert!(invalid.parse::<crate::Id<16>>().is_err());
        }
    }
}
