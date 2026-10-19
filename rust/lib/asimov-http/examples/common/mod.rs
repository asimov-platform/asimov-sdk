// This is free and unencumbered software released into the public domain.

use alloc::{boxed::Box, vec};
use asimov_http::{Component, SecretKey, VerificationPolicy};
use std::net::SocketAddr;

pub type BoxError = Box<dyn core::error::Error + Send + Sync>;

pub const RESPONSE_TAG: &str = "asimov-http-example-response";

pub fn address() -> Result<SocketAddr, BoxError> {
    let address = match std::env::var("ASIMOV_HTTP_EXAMPLE_ADDR") {
        Ok(address) => address,
        Err(std::env::VarError::NotPresent) => "127.0.0.1:3000".into(),
        Err(error) => return Err(error.into()),
    };
    Ok(address.parse()?)
}

// Fixed demo identities let the two programs run without keyring setup.
pub fn client_key() -> SecretKey {
    SecretKey::from_bytes(&[1; 32])
}

pub fn server_key() -> SecretKey {
    SecretKey::from_bytes(&[2; 32])
}

pub fn response_policy() -> Result<VerificationPolicy, asimov_http::Error> {
    let mut policy = VerificationPolicy::new(vec![
        Component::new("@status")?,
        Component::new("content-type")?,
        // Bind the reply to this signed request, including its fresh nonce.
        Component::parse(r#""signature";req;key="asimov""#)?,
    ]);
    policy.required_tag = Some(RESPONSE_TAG.into());
    // VerifyingBody requires a signed digest in either section. Pinning a
    // content-digest component here would prevent the server choosing placement.
    Ok(policy)
}
