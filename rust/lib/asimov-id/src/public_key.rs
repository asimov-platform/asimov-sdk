// This is free and unencumbered software released into the public domain.

//! ASIMOV public keys.

use crate::{KeyError, PUBLIC_KEY_LEN, PUBLIC_KEY_PREFIX, PublicKeyEncoding};
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::str::FromStr;
use derive_more::Display;

#[derive(Clone, Copy, Debug, Default, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display("ⒶY{}", bs58::encode(self.0).into_string())]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(try_from = "String", into = "String"))]
pub struct PublicKey(pub(crate) [u8; 32]);

impl PublicKey {
    pub const ZERO: Self = Self([0u8; 32]);

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn into_bytes(self) -> [u8; 32] {
        self.0
    }

    pub fn encode(&self, encoding: PublicKeyEncoding) -> Option<String> {
        use PublicKeyEncoding::*;
        Some(match encoding {
            Asimov => self.to_string(),
            Base58 => bs58::encode(self.0).into_string(),
            Near => {
                alloc::format!("ed25519:{}", bs58::encode(self.0).into_string())
            },
            #[cfg(feature = "base64")]
            Base64 => data_encoding::BASE64.encode(self.as_bytes()),
            #[cfg(feature = "base64")]
            Base64Url => data_encoding::BASE64URL_NOPAD.encode(self.as_bytes()),
            #[cfg(feature = "hex")]
            Hex => data_encoding::HEXLOWER.encode(self.as_bytes()),
            #[cfg(feature = "z32")]
            Z32 => data_encoding_macro::new_encoding! {
                symbols: "ybndrfg8ejkmcpqxot1uwisza345h769",
            }
            .encode(self.as_bytes()),
            _ => return None, // unsupported format
        })
    }
}

impl FromStr for PublicKey {
    type Err = KeyError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        if input.is_empty() {
            return Err(KeyError::EmptyInput);
        }
        if !PUBLIC_KEY_LEN.contains(&input.len()) {
            return Err(KeyError::InvalidLength);
        }
        let Some(input) = input.strip_prefix(PUBLIC_KEY_PREFIX) else {
            return Err(KeyError::InvalidPrefix);
        };
        let mut output = [0u8; 32];
        let count = bs58::decode(&input)
            .onto(&mut output)
            .map_err(|e| KeyError::InvalidEncoding(e))?;
        if count != output.len() {
            return Err(KeyError::InvalidLength);
        }
        Ok(Self(output))
    }
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl<T> From<&T> for PublicKey
where
    T: Clone + Into<Self>,
{
    fn from(t: &T) -> Self {
        t.clone().into()
    }
}

impl From<[u8; 32]> for PublicKey {
    fn from(input: [u8; 32]) -> Self {
        Self(input)
    }
}

/// Requires exactly 32 bytes; curve validity is checked on conversion to Iroh.
impl TryFrom<&[u8]> for PublicKey {
    type Error = KeyError;

    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self(input.try_into().map_err(|_| KeyError::InvalidLength)?))
    }
}

impl TryFrom<&Vec<u8>> for PublicKey {
    type Error = KeyError;

    fn try_from(input: &Vec<u8>) -> Result<Self, Self::Error> {
        Self::try_from(input.as_slice())
    }
}

#[cfg(feature = "ed25519-dalek")]
impl From<&ed25519_dalek::VerifyingKey> for PublicKey {
    fn from(input: &ed25519_dalek::VerifyingKey) -> Self {
        Self(input.as_bytes().clone())
    }
}

#[cfg(feature = "iroh")]
impl From<iroh::PublicKey> for PublicKey {
    fn from(input: iroh::PublicKey) -> Self {
        Self(input.as_bytes().clone())
    }
}

#[cfg(feature = "iroh")]
/// Rejects bytes that do not represent a valid Ed25519 curve point.
impl TryFrom<PublicKey> for iroh::PublicKey {
    type Error = iroh::KeyParsingError;

    fn try_from(input: PublicKey) -> Result<Self, Self::Error> {
        iroh::PublicKey::from_bytes(&input.into_bytes())
    }
}

#[cfg(feature = "iroh")]
impl From<iroh::EndpointAddr> for PublicKey {
    fn from(input: iroh::EndpointAddr) -> Self {
        Self(input.id.as_bytes().clone())
    }
}

#[cfg(feature = "iroh")]
/// Validates the endpoint's public key before constructing its address.
impl TryFrom<PublicKey> for iroh::EndpointAddr {
    type Error = iroh::KeyParsingError;

    fn try_from(input: PublicKey) -> Result<Self, Self::Error> {
        let endpoint_id = iroh::EndpointId::try_from(input)?;
        Ok(iroh::EndpointAddr::from(endpoint_id))
    }
}

// #[cfg(feature = "p2panda")]
// impl From<&p2panda_core::PublicKey> for PublicKey {
//     fn from(bytes: &p2panda_core::PublicKey) -> Self {
//         Self(bytes.as_bytes().clone())
//     }
// }

// #[cfg(feature = "p2panda")]
// impl TryInto<p2panda_core::PublicKey> for PublicKey {
//     type Error = p2panda_core::IdentityError;

//     fn try_into(self) -> Result<p2panda_core::PublicKey, Self::Error> {
//         p2panda_core::PublicKey::from_bytes(&self.into_bytes())
//     }
// }

impl TryFrom<String> for PublicKey {
    type Error = KeyError;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        Self::from_str(&input)
    }
}

impl From<PublicKey> for String {
    fn from(input: PublicKey) -> String {
        input.to_string()
    }
}

#[cfg(feature = "eloquent")]
impl eloquent::ToSql for PublicKey {
    fn to_sql(&self) -> Result<String, eloquent::error::EloquentError> {
        use alloc::format;
        let hex: String = self.0.iter().map(|b| format!("{b:02X}")).collect();
        Ok(format!("X'{hex}'"))
    }
}

#[cfg(feature = "libsql")]
impl libsql::params::IntoValue for PublicKey {
    fn into_value(self) -> libsql::Result<libsql::Value> {
        Ok(libsql::Value::Blob(self.0.to_vec()))
    }
}

#[cfg(feature = "rocket")]
impl<'r> rocket::request::FromParam<'r> for PublicKey {
    type Error = KeyError;

    fn from_param(input: &'r str) -> Result<Self, Self::Error> {
        Self::from_str(input)
    }
}

#[cfg(feature = "turso")]
impl turso::IntoValue for PublicKey {
    fn into_value(self) -> turso::Result<turso::Value> {
        Ok(turso::Value::Blob(self.0.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_conversions_require_exact_length() {
        for length in [0, 1, 31, 33, 64] {
            let bytes = alloc::vec![42; length];
            assert_eq!(PublicKey::try_from(&bytes), Err(KeyError::InvalidLength));
            assert_eq!(
                PublicKey::try_from(bytes.as_slice()),
                Err(KeyError::InvalidLength)
            );
        }

        for bytes in [[0; 32], [255; 32], core::array::from_fn(|i| i as u8)] {
            let expected = PublicKey::from(bytes);
            assert_eq!(PublicKey::try_from(&bytes.to_vec()), Ok(expected));
            assert_eq!(PublicKey::try_from(bytes.as_slice()), Ok(expected));
            assert_eq!(expected.to_string().parse::<PublicKey>(), Ok(expected));
        }
    }

    #[cfg(feature = "iroh")]
    #[test]
    fn iroh_conversions_validate_curve_points() {
        let key = iroh::SecretKey::from_bytes(&[42; 32]).public();
        let public_key = PublicKey::from(key);
        assert_eq!(iroh::PublicKey::try_from(public_key).unwrap(), key);
        assert_eq!(iroh::EndpointAddr::try_from(public_key).unwrap().id, key);

        // This compressed Edwards encoding does not decompress to a point.
        let invalid = PublicKey::from([2; 32]);
        assert!(iroh::PublicKey::try_from(invalid).is_err());
        assert!(iroh::EndpointAddr::try_from(invalid).is_err());
    }
}
