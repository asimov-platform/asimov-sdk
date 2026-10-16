// This is free and unencumbered software released into the public domain.

use crate::Error;
use alloc::{string::String, vec::Vec};
use http::{HeaderMap, HeaderValue};
use sfv::{Dictionary, FieldType, Parser, Version};

// Bound parser work independently of a particular HTTP transport's limits.
pub(crate) const MAX_FIELD_BYTES: usize = 64 * 1024;
pub(crate) const MAX_COMPONENTS: usize = 64;

pub(crate) fn parser(value: &str) -> Result<Parser<'_>, Error> {
    if value.len() > MAX_FIELD_BYTES {
        return Err(Error::Limit);
    }
    Ok(Parser::new(value).with_version(Version::Rfc8941))
}

pub(crate) fn dictionary(value: &str) -> Result<Dictionary, Error> {
    Ok(parser(value)?.parse()?)
}

pub(crate) fn header(value: String) -> Result<HeaderValue, Error> {
    if value.len() > MAX_FIELD_BYTES {
        return Err(Error::Limit);
    }
    HeaderValue::try_from(value).map_err(|_| Error::InvalidField("non-ASCII field value"))
}

pub(crate) fn values<'a>(maps: &[&'a HeaderMap], name: &str) -> Result<Vec<&'a [u8]>, Error> {
    let mut size = 0usize;
    let mut values = Vec::new();
    for map in maps {
        for value in map.get_all(name) {
            let value = value.as_bytes().trim_ascii();
            size = size.checked_add(value.len() + 2).ok_or(Error::Limit)?;
            if size > MAX_FIELD_BYTES {
                return Err(Error::Limit);
            }
            values.push(value);
        }
    }
    Ok(values)
}

pub(crate) fn combined(maps: &[&HeaderMap], name: &str) -> Result<Option<String>, Error> {
    let values = values(maps, name)?;
    if values.is_empty() {
        return Ok(None);
    }
    let mut result = String::new();
    for (i, value) in values.iter().enumerate() {
        if !value.is_ascii() || value.contains(&b'\r') || value.contains(&b'\n') {
            return Err(Error::InvalidField("non-ASCII or multiline field value"));
        }
        if i != 0 {
            result.push_str(", ");
        }
        result.push_str(core::str::from_utf8(value).map_err(|_| Error::InvalidField("UTF-8"))?);
    }
    Ok(Some(result))
}

pub(crate) fn serialize_entry(entry: &sfv::ListEntry) -> Result<String, Error> {
    match entry {
        sfv::ListEntry::Item(item) => Ok(item.serialize()),
        sfv::ListEntry::InnerList(inner) => alloc::vec![inner.clone().into()]
            .serialize()
            .ok_or(Error::InvalidField("empty structured list")),
    }
}
