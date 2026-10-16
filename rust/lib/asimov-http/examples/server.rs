// This is free and unencumbered software released into the public domain.

//! Run with `cargo run -p asimov-http --example server`.
//! Both HTTP/1.1 and cleartext HTTP/2 are accepted on the same loopback port.

extern crate alloc;

mod common;

use alloc::{collections::BTreeMap, format, string::String, sync::Arc};
use asimov_http::{
    Component, FieldSection, Message, SignatureInput, SignatureParams, Signer, VerificationPolicy,
    Verifier,
    body::{SigningBody, sign_buffered_response, verify_request},
    http::{Method, Request, Response, StatusCode},
    key_id,
};
use bytes::Bytes;
use common::BoxError;
use core::{convert::Infallible, time::Duration};
use http_body_util::{BodyExt as _, Full, Limited, StreamBody, combinators::UnsyncBoxBody};
use hyper::{
    body::{Frame, Incoming},
    service::service_fn,
};
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto::Builder,
};
use std::{net::SocketAddr, sync::Mutex};
use tokio::net::TcpListener;

type ResponseBody = UnsyncBoxBody<Bytes, BoxError>;
type Nonces = Arc<Mutex<BTreeMap<String, i64>>>;

const LINES: [&[u8]; 3] = [
    b"{\"sequence\":1}\n",
    b"{\"sequence\":2}\n",
    b"{\"sequence\":3}\n",
];

fn full(bytes: Bytes) -> ResponseBody {
    Full::new(bytes)
        .map_err(|never| match never {})
        .boxed_unsync()
}

async fn reply(
    request: Request<Incoming>,
    address: SocketAddr,
    nonces: Nonces,
) -> Result<Response<ResponseBody>, BoxError> {
    let headers = match (request.method(), request.uri().path()) {
        (&Method::POST, "/headers") => true,
        (&Method::POST, "/trailers") => false,
        _ => return Err("expected POST /headers or POST /trailers".into()),
    };
    // Use the configured authority and actual path/query, not an untrusted Host.
    let target = format!(
        "http://{address}{}",
        request
            .uri()
            .path_and_query()
            .ok_or("missing request target")?
    );
    let (parts, body) = request.into_parts();
    let bytes = Limited::new(body, 16 * 1024).collect().await?.to_bytes();
    let request = Request::from_parts(parts, bytes);
    let key = common::client_key().public();
    let policy = VerificationPolicy::request();
    let skew = i64::try_from(policy.clock_skew)?;
    let verifier = Verifier::new(key, &key_id(&key), "asimov", policy)?;
    let now = SignatureParams::now()?;
    let verified = verify_request(&request, &target, &verifier, now)?;
    // Authorize only the configured demo key. Consume its nonce atomically
    // after signature/body verification and before producing the response.
    {
        let mut seen = nonces
            .lock()
            .map_err(|_| std::io::Error::other("nonce cache poisoned"))?;
        seen.retain(|_, expires| *expires > now);
        let nonce = verified.input.parameter("nonce").ok_or("missing nonce")?;
        let expires = verified
            .input
            .integer_parameter("expires")
            .ok_or("missing expiry")?;
        if seen.contains_key(nonce) {
            return Err("replayed request".into());
        }
        seen.insert(nonce.into(), expires + skew);
    }
    println!(
        "Verified request: {:?} {}",
        request.version(),
        request.uri().path()
    );
    let context = Message::from_request(&request, &target)?;
    let key = common::server_key();
    let mut covered = common::response_policy()?.required_components;
    covered.push(if headers {
        Component::new("content-digest")?
    } else {
        Component::trailer("content-digest")?
    });
    let input = SignatureInput::new(
        covered,
        SignatureParams::fresh(&key.public(), common::RESPONSE_TAG)?,
    )?;
    let signer = Signer::new(key, "response", input)?;
    let response = Response::builder()
        .version(request.version())
        .header("content-type", "application/jsonl");
    if headers {
        let mut response = response.body(Bytes::from(LINES.concat()))?;
        sign_buffered_response(&mut response, Some(context), &signer)?;
        Ok(response.map(full))
    } else {
        let frames = async_stream::stream! {
            for line in LINES {
                tokio::time::sleep(Duration::from_millis(300)).await;
                yield Ok::<_, Infallible>(Frame::data(Bytes::from_static(line)));
            }
        };
        let response = response.body(StreamBody::new(frames))?;
        let response =
            SigningBody::response(response, Some(context), Some(signer), FieldSection::Headers)?;
        Ok(response.map(|body| body.map_err(Into::into).boxed_unsync()))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), BoxError> {
    let listener = TcpListener::bind(common::address()?).await?;
    let address = listener.local_addr()?;
    let nonces = Nonces::default();
    println!("Listening on http://{address} (HTTP/1.1 and HTTP/2)");
    loop {
        let (socket, _) = listener.accept().await?;
        let nonces = nonces.clone();
        tokio::spawn(async move {
            let service = service_fn(move |request| {
                let nonces = nonces.clone();
                async move {
                    let response = match reply(request, address, nonces).await {
                        Ok(response) => response,
                        Err(error) => {
                            eprintln!("Rejected request: {error}");
                            let mut response =
                                Response::new(full(Bytes::from_static(b"Request rejected\n")));
                            *response.status_mut() = StatusCode::BAD_REQUEST;
                            response
                        },
                    };
                    Ok::<_, Infallible>(response)
                }
            });
            if let Err(error) = Builder::new(TokioExecutor::new())
                .serve_connection(TokioIo::new(socket), service)
                .await
            {
                eprintln!("Connection failed: {error}");
            }
        });
    }
}
