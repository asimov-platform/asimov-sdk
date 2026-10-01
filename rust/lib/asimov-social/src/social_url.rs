// This is free and unencumbered software released into the public domain.

use crate::SocialLinkError;
use alloc::string::ToString;
use url::{ParseError, Url};

/// Parses an HTTP(S) social URL without credentials or a non-default port.
/// Normalizes to HTTPS, removing one leading `www.` and known host aliases.
pub(crate) fn parse(input: &str) -> Result<Url, SocialLinkError> {
    let mut url = Url::parse(input)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(SocialLinkError::UnsupportedScheme(url.scheme().to_string()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(SocialLinkError::SpuriousCredentials);
    }
    if let Some(port) = url.port() {
        return Err(SocialLinkError::SpuriousPort(port));
    }
    // Check the original scheme's default port before upgrading HTTP to HTTPS.
    url.set_scheme("https")
        .expect("HTTP(S) URLs support the HTTPS scheme");
    let host = url.host_str().ok_or(ParseError::EmptyHost)?;
    let canonical = host.strip_prefix("www.").unwrap_or(host);
    let canonical = match canonical {
        "discordapp.com" => "discord.com",
        "m.facebook.com" | "mbasic.facebook.com" => "facebook.com",
        "lu.ma" => "luma.com",
        "old.reddit.com" | "new.reddit.com" | "m.reddit.com" => "reddit.com",
        "telegram.me" | "telegram.dog" => "t.me",
        "m.twitch.tv" => "twitch.tv",
        "twitter.com" | "mobile.twitter.com" | "mobile.x.com" => "x.com",
        "m.youtube.com" => "youtube.com",
        host if host.strip_suffix(".linkedin.com").is_some_and(|country| {
            country.len() == 2 && country.bytes().all(|b| b.is_ascii_alphabetic())
        }) =>
        {
            "linkedin.com"
        },
        host => host,
    };
    if canonical != host {
        let host = canonical.to_string();
        url.set_host(Some(&host))?;
    }
    Ok(url)
}
