// This is free and unencumbered software released into the public domain.

use crate::{Id, IdClass, IdError};
use alloc::vec::Vec;
use core::{ops::RangeInclusive, str::FromStr};
use derive_more::Display;

#[derive(Clone, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct BlobId(pub(crate) Id<32>);

impl BlobId {
    /// Class prefix plus 32 base58 zero digits for an all-zero hash.
    pub const ID_LEN_MIN: usize = 1 + 32;
    /// Class prefix plus ceil(log58(256^32)) base58 digits.
    pub const ID_LEN_MAX: usize = 1 + 44;
    pub const ID_LEN: RangeInclusive<usize> = Self::ID_LEN_MIN..=Self::ID_LEN_MAX;
    /// Lexical bounds; use parsing to validate the decoded payload length.
    pub const PATTERN: &'static str = "^B[1-9A-HJ-NP-Za-km-z]{32,44}$";

    pub fn as_id(&self) -> &Id<32> {
        &self.0
    }

    pub fn into_id(self) -> Id<32> {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn encoded_hashes_fit_advertised_bounds() {
        assert_eq!(BlobId::from([0; 32]).to_string().len(), BlobId::ID_LEN_MIN);
        assert_eq!(
            BlobId::from([255; 32]).to_string().len(),
            BlobId::ID_LEN_MAX
        );

        // A fixed seed makes varied payload coverage reproducible.
        let mut state = 0x1234_5678_u32;
        for _ in 0..256 {
            let bytes = core::array::from_fn(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            });
            let id = BlobId::from(bytes);
            let encoded = id.to_string();
            assert!(BlobId::ID_LEN.contains(&encoded.len()));
            assert_eq!(encoded.parse::<BlobId>().unwrap(), id);
        }
    }
}

impl FromStr for BlobId {
    type Err = IdError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let id = Id::from_str(input)?;
        if id.class() != IdClass::Blob {
            return Err(IdError::UnknownClass);
        }
        Ok(Self(id))
    }
}

impl From<[u8; 32]> for BlobId {
    fn from(bytes: [u8; 32]) -> Self {
        Self(Id::from((IdClass::Blob, bytes)))
    }
}

impl From<&[u8; 32]> for BlobId {
    fn from(bytes: &[u8; 32]) -> Self {
        Self(Id::from((IdClass::Blob, bytes.clone())))
    }
}

impl From<&Vec<u8>> for BlobId {
    fn from(bytes: &Vec<u8>) -> Self {
        Self(Id::from((IdClass::Blob, bytes)))
    }
}

#[cfg(feature = "async-graphql")]
impl async_graphql::ScalarType for BlobId {
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
impl async_graphql::InputType for BlobId {
    type RawValueType = Self;

    fn type_name() -> alloc::borrow::Cow<'static, str> {
        "BLOB ID".into()
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

#[cfg(feature = "iroh")]
impl From<&iroh_blobs::Hash> for BlobId {
    fn from(bytes: &iroh_blobs::Hash) -> Self {
        Self(Id::from((IdClass::Blob, bytes.as_bytes().clone())))
    }
}

#[cfg(feature = "p2panda")]
impl From<&p2panda_core::Hash> for BlobId {
    fn from(bytes: &p2panda_core::Hash) -> Self {
        Self(Id::from((IdClass::Blob, bytes.as_bytes().clone())))
    }
}

#[cfg(feature = "iroh")]
impl Into<iroh_blobs::Hash> for BlobId {
    fn into(self) -> iroh_blobs::Hash {
        iroh_blobs::Hash::from(self.into_id().into_bytes())
    }
}

#[cfg(feature = "p2panda")]
impl Into<p2panda_core::Hash> for BlobId {
    fn into(self) -> p2panda_core::Hash {
        p2panda_core::Hash::from(self.into_id().into_bytes())
    }
}

#[cfg(feature = "eloquent")]
impl eloquent::ToSql for BlobId {
    fn to_sql(&self) -> Result<alloc::string::String, eloquent::error::EloquentError> {
        self.as_id().to_sql()
    }
}

#[cfg(feature = "libsql")]
impl libsql::params::IntoValue for BlobId {
    fn into_value(self) -> libsql::Result<libsql::Value> {
        self.into_id().into_value()
    }
}

#[cfg(feature = "rocket")]
impl<'r> rocket::request::FromParam<'r> for BlobId {
    type Error = IdError;

    fn from_param(input: &'r str) -> Result<Self, Self::Error> {
        Self::from_str(input)
    }
}

#[cfg(feature = "turso")]
impl turso::IntoValue for BlobId {
    fn into_value(self) -> turso::Result<turso::Value> {
        self.into_id().into_value()
    }
}
