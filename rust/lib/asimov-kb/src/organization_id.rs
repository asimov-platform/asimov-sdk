// This is free and unencumbered software released into the public domain.

use crate::{Id, IdClass, IdError};
use alloc::vec::Vec;
use core::{ops::RangeInclusive, str::FromStr};
use derive_more::Display;

#[derive(Clone, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde_with::DeserializeFromStr, serde::Serialize)
)]
pub struct OrganizationId(pub(crate) Id<16>);

impl OrganizationId {
    pub const ID_LEN_MIN: usize = 1 + 16;
    pub const ID_LEN_MAX: usize = 1 + 22;
    pub const ID_LEN: RangeInclusive<usize> = Self::ID_LEN_MIN..=Self::ID_LEN_MAX;
    pub const PATTERN: &'static str = "^O[1-9A-HJ-NP-Za-km-z]{16,22}$";

    #[cfg(feature = "uuid")]
    pub fn new() -> Self {
        Self(Id::new_uuid(IdClass::Organization))
    }

    pub fn as_id(&self) -> &Id {
        &self.0
    }

    pub fn into_id(self) -> Id {
        self.0
    }
}

impl FromStr for OrganizationId {
    type Err = IdError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let id = Id::from_str(input)?;
        if id.class() != IdClass::Organization {
            return Err(IdError::UnknownClass);
        }
        Ok(Self(id))
    }
}

impl From<[u8; 16]> for OrganizationId {
    fn from(bytes: [u8; 16]) -> Self {
        Self(Id::from((IdClass::Organization, bytes)))
    }
}

impl From<&Vec<u8>> for OrganizationId {
    fn from(bytes: &Vec<u8>) -> Self {
        Self(Id::from((IdClass::Organization, bytes)))
    }
}

#[cfg(feature = "async-graphql")]
impl async_graphql::ScalarType for OrganizationId {
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
impl async_graphql::InputType for OrganizationId {
    type RawValueType = Self;

    fn type_name() -> alloc::borrow::Cow<'static, str> {
        "ORGANIZATION ID".into()
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
impl eloquent::ToSql for OrganizationId {
    fn to_sql(&self) -> Result<alloc::string::String, eloquent::error::EloquentError> {
        self.as_id().to_sql()
    }
}

#[cfg(feature = "libsql")]
impl libsql::params::IntoValue for OrganizationId {
    fn into_value(self) -> libsql::Result<libsql::Value> {
        self.into_id().into_value()
    }
}

#[cfg(feature = "rocket")]
impl<'r> rocket::request::FromParam<'r> for OrganizationId {
    type Error = IdError;

    fn from_param(input: &'r str) -> Result<Self, Self::Error> {
        Self::from_str(input)
    }
}

#[cfg(feature = "turso")]
impl turso::IntoValue for OrganizationId {
    fn into_value(self) -> turso::Result<turso::Value> {
        self.into_id().into_value()
    }
}
