// This is free and unencumbered software released into the public domain.

use crate::SocialLinkError;
use alloc::string::ToString;
use url::{ParseError, Url};

/// Parses an HTTPS social URL without credentials or a non-default port.
/// Normalizes the authority, including removal of one leading `www.`.
pub(crate) fn parse(input: &str) -> Result<Url, SocialLinkError> {
    let mut url = Url::parse(input)?;
    if url.scheme() != "https" {
        return Err(SocialLinkError::UnsupportedScheme(url.scheme().to_string()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(SocialLinkError::SpuriousCredentials);
    }
    if let Some(port) = url.port() {
        return Err(SocialLinkError::SpuriousPort(port));
    }
    let host = url.host_str().ok_or(ParseError::EmptyHost)?;
    if let Some(host) = host.strip_prefix("www.") {
        let host = host.to_string();
        url.set_host(Some(&host))?;
    }
    Ok(url)
}
