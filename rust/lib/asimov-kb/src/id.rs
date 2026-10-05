// This is free and unencumbered software released into the public domain.

use crate::{IdClass, IdError};
use alloc::vec::Vec;
use core::str::FromStr;
use derive_more::Display;

#[derive(Clone, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde_with::SerializeDisplay, serde_with::DeserializeFromStr)
)]
#[display("{class}{}", bs58::encode(bytes).into_string())]
pub struct Id<const N: usize = 16> {
    pub(crate) class: IdClass,
    pub(crate) bytes: [u8; N],
}

impl<const N: usize> Id<N> {
    pub fn zero(class: IdClass) -> Self {
        Self {
            class,
            bytes: [0u8; N],
        }
    }

    pub fn class(&self) -> IdClass {
        self.class
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    pub fn into_bytes(self) -> [u8; N] {
        self.bytes
    }

    #[cfg(feature = "std")]
    pub fn yaml_path(&self) -> std::path::PathBuf {
        self.dir_path().join(self.class().yaml_path())
    }

    #[cfg(feature = "std")]
    pub fn dir_path(&self) -> std::path::PathBuf {
        use alloc::format;
        self.class()
            .dir_path()
            .join(format!("{}/{}", self.shard(), self))
    }

    #[cfg(feature = "std")]
    fn shard(&self) -> alloc::string::String {
        use alloc::string::String;
        let id_str = bs58::encode(self.bytes).into_string();
        let id_len = id_str.chars().count();
        id_str
            .chars()
            .skip(id_len.saturating_sub(2))
            .collect::<String>()
            .to_lowercase()
    }
}

#[cfg(feature = "uuid")]
impl Id<16> {
    pub fn new_uuid(class: IdClass) -> Self {
        Self {
            class,
            bytes: uuid::Uuid::now_v7().into_bytes(),
        }
    }

    pub fn as_uuid(&self) -> uuid::Uuid {
        uuid::Uuid::from_bytes(self.bytes)
    }

    pub fn into_uuid(self) -> uuid::Uuid {
        uuid::Uuid::from_bytes(self.bytes)
    }
}

impl<const N: usize> From<(IdClass, [u8; N])> for Id<N> {
    fn from((class, bytes): (IdClass, [u8; N])) -> Self {
        Self { class, bytes }
    }
}

/// Requires exactly `N` payload bytes, without padding or truncation.
impl<const N: usize> TryFrom<(IdClass, &[u8])> for Id<N> {
    type Error = IdError;

    fn try_from((class, bytes): (IdClass, &[u8])) -> Result<Self, Self::Error> {
        let bytes = bytes.try_into().map_err(|_| IdError::InvalidLength)?;
        Ok(Self { class, bytes })
    }
}

impl<const N: usize> TryFrom<(IdClass, &Vec<u8>)> for Id<N> {
    type Error = IdError;

    fn try_from((class, bytes): (IdClass, &Vec<u8>)) -> Result<Self, Self::Error> {
        Self::try_from((class, bytes.as_slice()))
    }
}

impl<const N: usize> FromStr for Id<N> {
    type Err = IdError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let class = IdClass::from_str(input.get(..1).ok_or(IdError::UnknownClass)?)?;
        let mut id = Id::zero(class);
        use bs58::decode::Error::*;
        match bs58::decode(&input[1..]).onto(&mut id.bytes) {
            Ok(len) => {
                if len == id.bytes.len() {
                    Ok(id)
                } else {
                    Err(IdError::InvalidLength)
                }
            },
            Err(err) => Err(match err {
                BufferTooSmall => IdError::InvalidLength,
                InvalidCharacter { .. } | NonAsciiCharacter { .. } => IdError::InvalidEncoding,
                _ => unreachable!(),
            }),
        }
    }
}

#[cfg(feature = "async-graphql")]
impl<const N: usize> async_graphql::ScalarType for Id<N> {
    fn parse(value: async_graphql::Value) -> async_graphql::InputValueResult<Self> {
        match value {
            async_graphql::Value::String(s) => Ok(Self::from_str(&s)?),
            _ => Err(async_graphql::InputValueError::expected_type(value)),
        }
    }

    fn is_valid(value: &async_graphql::Value) -> bool {
        matches!(value, async_graphql::Value::String(_))
    }

    fn to_value(&self) -> async_graphql::Value {
        async_graphql::Value::String(alloc::string::ToString::to_string(self))
    }
}

#[cfg(feature = "async-graphql")]
impl<const N: usize> async_graphql::InputType for Id<N> {
    type RawValueType = Self;

    fn type_name() -> alloc::borrow::Cow<'static, str> {
        use alloc::format;
        format!("ID<{}>", N).into()
    }

    fn create_type_info(registry: &mut async_graphql::registry::Registry) -> alloc::string::String {
        <alloc::string::String as async_graphql::InputType>::create_type_info(registry)
    }

    fn parse(value: Option<async_graphql::Value>) -> async_graphql::InputValueResult<Self> {
        <Self as async_graphql::ScalarType>::parse(value.unwrap_or_default())
    }

    fn to_value(&self) -> async_graphql::Value {
        <Self as async_graphql::ScalarType>::to_value(self)
    }

    fn as_raw_value(&self) -> Option<&Self::RawValueType> {
        Some(self)
    }
}

#[cfg(feature = "eloquent")]
impl<const N: usize> eloquent::ToSql for Id<N> {
    fn to_sql(&self) -> Result<alloc::string::String, eloquent::error::EloquentError> {
        use alloc::{format, string::String};
        let hex: String = self.bytes.iter().map(|b| format!("{b:02X}")).collect();
        Ok(format!("X'{hex}'"))
    }
}

#[cfg(feature = "libsql")]
impl<const N: usize> libsql::params::IntoValue for Id<N> {
    fn into_value(self) -> libsql::Result<libsql::Value> {
        Ok(libsql::Value::Blob(self.bytes.to_vec()))
    }
}

#[cfg(feature = "rocket")]
impl<'r, const N: usize> rocket::request::FromParam<'r> for Id<N> {
    type Error = IdError;

    fn from_param(input: &'r str) -> Result<Self, Self::Error> {
        Self::from_str(input)
    }
}

#[cfg(feature = "turso")]
impl<const N: usize> turso::IntoValue for Id<N> {
    fn into_value(self) -> turso::Result<turso::Value> {
        Ok(turso::Value::Blob(self.bytes.to_vec()))
    }
}
