// This is free and unencumbered software released into the public domain.

//! String representations shared with the remote HTTP protocol.

use alloc::string::{String, ToString};
use core::{fmt::Display, str::FromStr, time::Duration};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

pub(crate) mod optional_string {
    use super::*;

    pub fn serialize<T: Display, S: Serializer>(
        value: &Option<T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value
            .as_ref()
            .map(ToString::to_string)
            .serialize(serializer)
    }

    pub fn deserialize<'de, T: FromStr, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<T>, D::Error>
    where
        T::Err: Display,
    {
        Option::<String>::deserialize(deserializer)?
            .map(|value| value.parse().map_err(D::Error::custom))
            .transpose()
    }
}

pub(crate) mod optional_duration {
    use super::*;

    pub fn serialize<S: Serializer>(
        value: &Option<Duration>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value
            .map(|value| humantime::format_duration(value).to_string())
            .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Duration>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|value| humantime::parse_duration(&value).map_err(D::Error::custom))
            .transpose()
    }
}
