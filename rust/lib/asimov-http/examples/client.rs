// This is free and unencumbered software released into the public domain.

//! Run against the server example: `cargo run -p asimov-http --example client`.
//! Add `-- --headers` for a header digest, `-- --http2` for HTTP/2, or both.

extern crate alloc;

mod common;

use alloc::{
    format,
    string::{String, ToString},
};
use asimov_http::{
    Message, REQUEST_SIGNATURE_TAG, SignatureInput, SignatureParams, Signer, VerificationPolicy,
    Verifier,
    body::{VerifyingBody, sign_request},
    http::{Request, Version},
    key_id,
};
use bytes::Bytes;
use common::BoxError;
use http_body_util::{BodyExt as _, Full};
use hyper::client::conn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::TcpStream;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), BoxError> {
    let mut http2 = false;
    let mut path = "/trailers";
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--http2" => http2 = true,
            "--headers" => path = "/headers",
            _ => return Err("usage: client [--http2] [--headers]".into()),
        }
    }
    let address = common::address()?;
    let target = format!("http://{address}{path}");
    let version = if http2 {
        Version::HTTP_2
    } else {
        Version::HTTP_11
    };
    let key = common::client_key();
    let input = SignatureInput::new(
        VerificationPolicy::request().required_components,
        SignatureParams::fresh(&key.public(), REQUEST_SIGNATURE_TAG)?,
    )?;
    let signer = Signer::new(key, "asimov", input)?;
    let mut request = Request::post(if http2 { target.as_str() } else { path })
        .version(version)
        .header("host", address.to_string())
        .header("authorization", "Bearer example")
        .header("accept", "application/jsonl")
        .header("content-type", "application/json")
        .header("te", "trailers")
        .body(Bytes::from_static(br#"{"message":"hello"}"#))?;
    if !http2 {
        request.headers_mut().insert("connection", "te".parse()?);
    }
    sign_request(&mut request, &target, &signer)?;
    let context = Message::from_request(&request, &target)?;
    let request = request.map(Full::new);

    let socket = TokioIo::new(TcpStream::connect(address).await?);
    let response = if http2 {
        let (mut sender, connection) = conn::http2::handshake(TokioExecutor::new(), socket).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                eprintln!("Connection failed: {error}");
            }
        });
        sender.send_request(request).await?
    } else {
        let (mut sender, connection) = conn::http1::handshake(socket).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                eprintln!("Connection failed: {error}");
            }
        });
        sender.send_request(request).await?
    };
    if !response.status().is_success() {
        return Err(format!("server returned {}", response.status()).into());
    }
    println!("Received {:?} response", response.version());
    let key = common::server_key().public();
    let verifier = Verifier::new(key, &key_id(&key), "response", common::response_policy()?)?;
    let mut body = VerifyingBody::response(
        response,
        Some(context),
        Some(verifier),
        SignatureParams::now()?,
    )?
    .into_body();
    while let Some(frame) = body.frame().await {
        if let Ok(data) = frame?.into_data() {
            println!("Provisional: {}", String::from_utf8_lossy(&data).trim_end());
        }
    }
    assert!(body.is_verified());
    println!("Verified SHA-256 digest and Ed25519 signature at EOF.");
    Ok(())
}
