// This is free and unencumbered software released into the public domain.

use alloc::sync::Arc;
use rustls_platform_verifier::BuilderVerifierExt;

pub(super) fn tls_config() -> Result<rustls::ClientConfig, rustls::Error> {
    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_platform_verifier()
        .map(|builder| builder.with_no_client_auth())
}

pub(super) fn client_builder() -> Result<reqwest::ClientBuilder, rustls::Error> {
    Ok(reqwest::Client::builder()
        .no_hickory_dns()
        .connect_timeout(core::time::Duration::from_secs(5))
        .tls_backend_preconfigured(tls_config()?))
}
