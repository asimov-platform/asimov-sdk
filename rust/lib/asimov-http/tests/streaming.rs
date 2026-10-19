// This is free and unencumbered software released into the public domain.

#![cfg(feature = "integrity")]

extern crate alloc;

use alloc::{collections::VecDeque, format, sync::Arc, vec, vec::Vec};
use asimov_http::{
    Component, ContentDigest, FieldSection, Message, SecretKey, SignatureInput, SignatureParams,
    Signer, VerificationPolicy, Verifier,
    body::{BodyError, SigningBody, VerifyingBody, sign_request, verify_request},
    key_id,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bytes::Bytes;
use core::{
    cell::Cell,
    convert::Infallible,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use http::{HeaderMap, Request, Response, StatusCode, Version};
use http_body::{Body, Frame};
use http_body_util::{BodyExt as _, Full};
use hyper::{body::Incoming, service::service_fn};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::oneshot,
    time::timeout,
};

const NOW: i64 = 100;
const DEADLINE: Duration = Duration::from_secs(10);

// Deliberately !Sync: adapters must work with Axum-style bodies as well.
struct Frames {
    frames: VecDeque<Result<Frame<Bytes>, std::io::Error>>,
    _not_sync: Cell<()>,
}

impl Frames {
    fn new(frames: Vec<Result<Frame<Bytes>, std::io::Error>>) -> Self {
        Self {
            frames: frames.into(),
            _not_sync: Cell::new(()),
        }
    }
}

impl Body for Frames {
    type Data = Bytes;
    type Error = std::io::Error;
    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        Poll::Ready(self.get_mut().frames.pop_front())
    }
}

fn keys(components: Vec<Component>) -> (Signer, Verifier) {
    let key = SecretKey::from_bytes(&[7; 32]);
    let id = key_id(&key.public());
    let input = SignatureInput::new(
        components.clone(),
        SignatureParams {
            created: NOW,
            expires: Some(NOW + 60),
            nonce: Some(STANDARD.encode([1; 16])),
            key_id: id.clone(),
            tag: Some("test".into()),
        },
    )
    .unwrap();
    let mut policy = VerificationPolicy::new(components);
    policy.required_tag = Some("test".into());
    let verifier = Verifier::new(key.public(), &id, "s", policy).unwrap();
    (Signer::new(key, "s", input).unwrap(), verifier)
}

fn components(digest: FieldSection) -> Vec<Component> {
    vec![
        Component::new("@status").unwrap(),
        Component::new("content-type").unwrap(),
        match digest {
            FieldSection::Headers => Component::new("content-digest"),
            FieldSection::Trailers => Component::trailer("content-digest"),
        }
        .unwrap(),
    ]
}

#[tokio::test]
async fn all_header_trailer_combinations_verify_without_buffering_data() {
    for digest_section in [FieldSection::Headers, FieldSection::Trailers] {
        for input_section in [FieldSection::Headers, FieldSection::Trailers] {
            for signature_section in [FieldSection::Headers, FieldSection::Trailers] {
                let (signer, verifier) = keys(components(digest_section));
                let mut message =
                    Message::response(StatusCode::OK, HeaderMap::new(), None).unwrap();
                message
                    .headers
                    .insert("content-type", "application/jsonl".parse().unwrap());
                message.fields_mut(digest_section).insert(
                    "content-digest",
                    ContentDigest::new(b"first\nlast\n")
                        .to_field_value()
                        .parse()
                        .unwrap(),
                );
                let signature = signer.sign(&message).unwrap();
                message
                    .fields_mut(input_section)
                    .insert("signature-input", signature.signature_input);
                message
                    .fields_mut(signature_section)
                    .insert("signature", signature.signature);
                let mut frames = vec![
                    Ok(Frame::data(Bytes::from_static(b"first\n"))),
                    Ok(Frame::data(Bytes::from_static(b"last\n"))),
                ];
                if !message.trailers.is_empty() {
                    frames.push(Ok(Frame::trailers(message.trailers.clone())));
                }
                let mut body =
                    VerifyingBody::new(Frames::new(frames), message, Some(verifier), NOW);
                assert!(!body.is_end_stream());
                for expected in [b"first\n".as_slice(), b"last\n"] {
                    let frame = body.frame().await.unwrap().unwrap();
                    assert_eq!(frame.into_data().unwrap(), expected);
                    assert!(!body.is_verified());
                }
                while let Some(frame) = body.frame().await {
                    assert!(frame.unwrap().is_trailers());
                }
                assert!(body.is_verified());
                assert!(body.is_end_stream());
                assert!(body.verified_signature().is_some());
            }
        }
    }
}

#[tokio::test]
async fn empty_streams_emit_completion_and_preserve_existing_trailers() {
    for input_section in [FieldSection::Headers, FieldSection::Trailers] {
        let (signer, verifier) = keys(components(FieldSection::Trailers));
        let mut trailers = HeaderMap::new();
        trailers.insert("x-result", "finished".parse().unwrap());
        let response = Response::builder()
            .header("content-type", "application/jsonl")
            .header("content-length", "0")
            .header("trailer", "x-result")
            .body(Frames::new(vec![Ok(Frame::trailers(trailers))]))
            .unwrap();
        let response = SigningBody::response(response, None, Some(signer), input_section).unwrap();
        assert!(!response.headers().contains_key("content-length"));
        assert!(
            response.headers()["trailer"]
                .to_str()
                .unwrap()
                .contains("x-result")
        );
        assert!(!response.body().is_end_stream());
        assert_eq!(response.body().size_hint().exact(), None);
        let mut body = VerifyingBody::response(response, None, Some(verifier), NOW)
            .unwrap()
            .into_body();
        let frame = body.frame().await.unwrap().unwrap();
        let trailers = frame.into_trailers().unwrap();
        assert_eq!(trailers["x-result"], "finished");
        assert_eq!(
            trailers["content-digest"],
            "sha-256=:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=:"
        );
        assert!(body.is_verified());
        assert!(body.frame().await.is_none());
    }
}

#[tokio::test]
async fn missing_tampered_truncated_and_out_of_order_bodies_never_verify() {
    for failure in 0..7 {
        let (signer, verifier) = keys(components(FieldSection::Trailers));
        let mut message = Message::response(StatusCode::OK, HeaderMap::new(), None).unwrap();
        message
            .headers
            .insert("content-type", "application/jsonl".parse().unwrap());
        message.trailers.insert(
            "content-digest",
            ContentDigest::new(b"data\n")
                .to_field_value()
                .parse()
                .unwrap(),
        );
        signer
            .sign(&message)
            .unwrap()
            .append_to(&mut message.trailers)
            .unwrap();
        let mut trailers = message.trailers.clone();
        if failure == 2 {
            trailers.insert(
                "content-digest",
                ContentDigest::new(b"changed")
                    .to_field_value()
                    .parse()
                    .unwrap(),
            );
        }
        if failure == 3 {
            trailers.insert("signature", "s=:AQ==:".parse().unwrap());
        }
        let data = if failure == 1 {
            b"changed\n".as_slice()
        } else {
            b"data\n"
        };
        let mut frames = vec![Ok(Frame::data(Bytes::copy_from_slice(data)))];
        if failure != 0 {
            frames.push(Ok(Frame::trailers(trailers)));
        }
        if failure == 4 {
            frames.push(Ok(Frame::data(Bytes::from_static(b"late"))));
        }
        if failure == 5 {
            frames.push(Ok(Frame::trailers(HeaderMap::new())));
        }
        if failure == 6 {
            frames.push(Err(std::io::Error::other("truncated")));
        }
        let mut body = VerifyingBody::new(Frames::new(frames), message, Some(verifier), NOW);
        assert!(body.frame().await.unwrap().unwrap().is_data());
        assert!(body.frame().await.unwrap().is_err(), "case {failure}");
        assert!(!body.is_verified());
        assert!(body.verified_signature().is_none());
        assert!(body.frame().await.is_none());
    }
}

#[tokio::test]
async fn signer_propagates_source_failures_without_completion_fields() {
    let response = Response::new(Frames::new(vec![
        Ok(Frame::data(Bytes::from_static(b"data"))),
        Err(std::io::Error::other("interrupted")),
    ]));
    let mut body = SigningBody::response(response, None, None, FieldSection::Headers)
        .unwrap()
        .into_body();
    assert!(body.frame().await.unwrap().unwrap().is_data());
    assert!(matches!(body.frame().await, Some(Err(BodyError::Inner(_)))));
    assert!(body.frame().await.is_none());
}

#[tokio::test]
async fn an_unsigned_sha256_trailer_cannot_bypass_a_signed_unknown_digest() {
    let (signer, verifier) = keys(components(FieldSection::Headers));
    let mut message = Message::response(StatusCode::OK, HeaderMap::new(), None).unwrap();
    message
        .headers
        .insert("content-type", "application/jsonl".parse().unwrap());
    message
        .headers
        .insert("content-digest", "unknown=:AQ==:".parse().unwrap());
    signer
        .sign(&message)
        .unwrap()
        .append_to(&mut message.headers)
        .unwrap();
    let mut trailers = HeaderMap::new();
    trailers.insert(
        "content-digest",
        ContentDigest::new(b"data")
            .to_field_value()
            .parse()
            .unwrap(),
    );
    let frames = Frames::new(vec![
        Ok(Frame::data(Bytes::from_static(b"data"))),
        Ok(Frame::trailers(trailers)),
    ]);
    let mut body = VerifyingBody::new(frames, message, Some(verifier), NOW);
    assert!(body.frame().await.unwrap().is_ok());
    assert!(body.frame().await.unwrap().is_err());
    assert!(!body.is_verified());
}

struct GatedBody {
    first: bool,
    gate: oneshot::Receiver<()>,
    last: bool,
}

impl Body for GatedBody {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();
        if !this.first {
            this.first = true;
            return Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(b"first\n")))));
        }
        if !this.last {
            if Pin::new(&mut this.gate).poll(cx).is_pending() {
                return Poll::Pending;
            }
            this.last = true;
            return Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(b"last\n")))));
        }
        Poll::Ready(None)
    }
}

async fn send(
    version: Version,
    socket: TcpStream,
    request: Request<Full<Bytes>>,
) -> Response<Incoming> {
    if version == Version::HTTP_2 {
        let (mut sender, connection) =
            hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(socket))
                .await
                .unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });
        sender.send_request(request).await.unwrap()
    } else {
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(socket))
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });
        sender.send_request(request).await.unwrap()
    }
}

#[tokio::test]
async fn http1_and_http2_sign_requests_and_stream_signed_response_trailers() {
    for version in [Version::HTTP_11, Version::HTTP_2] {
        for input_section in [FieldSection::Headers, FieldSection::Trailers] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let url = format!("http://{address}/stream?x=1");
            let (request_signer, request_verifier) =
                keys(VerificationPolicy::request().required_components);
            let mut covered = components(FieldSection::Trailers);
            for value in [
                r#""@method";req"#,
                r#""@target-uri";req"#,
                r#""content-digest";req"#,
            ] {
                covered.push(Component::parse(value).unwrap());
            }
            let (response_signer, response_verifier) = keys(covered);
            let mut request = Request::post(&url)
                .version(version)
                .header("host", address.to_string())
                .header("accept", "application/jsonl")
                .header("authorization", "Bearer test")
                .header("content-type", "application/json")
                .header("te", "trailers")
                .body(Bytes::from_static(b"{}"))
                .unwrap();
            if version == Version::HTTP_11 {
                request
                    .headers_mut()
                    .insert("connection", "te".parse().unwrap());
            }
            sign_request(&mut request, &url, &request_signer).unwrap();
            let request_context = Message::from_request(&request, &url).unwrap();
            if version == Version::HTTP_11 {
                *request.uri_mut() = "/stream?x=1".parse().unwrap();
            }
            let (resume, gate) = oneshot::channel();
            let gate = Arc::new(std::sync::Mutex::new(Some(gate)));
            let server_url = url.clone();
            let server = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                let service = service_fn(move |request: Request<Incoming>| {
                    let gate = gate.lock().unwrap().take().unwrap();
                    let url = server_url.clone();
                    let verifier = request_verifier.clone();
                    let signer = response_signer.clone();
                    async move {
                        assert_eq!(request.version(), version);
                        let (parts, body) = request.into_parts();
                        let request =
                            Request::from_parts(parts, body.collect().await.unwrap().to_bytes());
                        verify_request(&request, &url, &verifier, NOW).unwrap();
                        let context = Message::from_request(&request, &url).unwrap();
                        let response = Response::builder()
                            .version(version)
                            .header("content-type", "application/jsonl")
                            .body(GatedBody {
                                first: false,
                                gate,
                                last: false,
                            })
                            .unwrap();
                        Ok::<_, Infallible>(
                            SigningBody::response(
                                response,
                                Some(context),
                                Some(signer),
                                input_section,
                            )
                            .unwrap(),
                        )
                    }
                });
                if version == Version::HTTP_2 {
                    hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                        .serve_connection(TokioIo::new(socket), service)
                        .await
                        .unwrap();
                } else {
                    hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(socket), service)
                        .await
                        .unwrap();
                }
            });
            let response = timeout(
                DEADLINE,
                send(
                    version,
                    TcpStream::connect(address).await.unwrap(),
                    request.map(Full::new),
                ),
            )
            .await
            .unwrap();
            assert_eq!(response.version(), version);
            let mut body = VerifyingBody::response(
                response,
                Some(request_context),
                Some(response_verifier),
                NOW,
            )
            .unwrap()
            .into_body();
            let first = timeout(DEADLINE, body.frame())
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .into_data()
                .unwrap();
            assert_eq!(first, "first\n"); // The server is still waiting for resume.
            assert!(!body.is_verified());
            resume.send(()).unwrap();
            let mut tail = Vec::new();
            let mut trailers_seen = false;
            while let Some(frame) = timeout(DEADLINE, body.frame()).await.unwrap() {
                let frame = frame.unwrap();
                if let Some(data) = frame.data_ref() {
                    tail.extend_from_slice(data);
                }
                if let Some(trailers) = frame.trailers_ref() {
                    assert!(trailers.contains_key("signature"));
                    assert!(trailers.contains_key("content-digest"));
                    trailers_seen = true;
                }
            }
            assert_eq!(tail, b"last\n");
            assert!(trailers_seen && body.is_verified());
            server.abort();
            let _ = server.await;
        }
    }
}
