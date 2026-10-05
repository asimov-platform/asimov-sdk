// This is free and unencumbered software released into the public domain.

use alloc::string::String;

/// Proxy configuration and initialization failures, without credentials.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The proxy URL could not be parsed.
    #[error("invalid proxy URL: {0}")]
    InvalidProxyUrl(#[from] url::ParseError),

    /// The proxy URL has no host.
    #[error("proxy URL is missing a host")]
    MissingProxyHost,

    /// The percent-decoded username is not UTF-8.
    #[error("proxy username is not valid UTF-8")]
    InvalidProxyUsername,

    /// The percent-decoded password is not UTF-8.
    #[error("proxy password is not valid UTF-8")]
    InvalidProxyPassword,

    /// The URL scheme is not HTTP, HTTPS, SOCKS5, or SOCKS5H.
    #[error("unsupported proxy scheme: {0}")]
    UnsupportedProxyScheme(String),

    /// The upstream API key is empty or whitespace-only.
    #[error("API key must be set and nonempty")]
    MissingApiKey,

    /// The upstream API key cannot be used in an HTTP header.
    #[error("API key contains invalid HTTP header bytes")]
    InvalidApiKey,

    /// Native TLS trust roots could not be loaded.
    #[cfg(feature = "std")]
    #[error("failed to load native TLS roots: {0}")]
    NativeRoots(#[from] std::io::Error),

    /// The TLS configuration could not be initialized.
    #[cfg(feature = "std")]
    #[error("failed to configure TLS: {0}")]
    Tls(#[from] rustls::Error),
}
