// This is free and unencumbered software released into the public domain.

use super::*;
use alloc::sync::Arc;
use asimov_flow::Bytes;
use asimov_http::{
    FieldSection,
    body::{SigningBody, verify_request},
    key_id,
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use http_body_util::BodyExt as _;
use hyper::{
    Request, Response, Version,
    body::{Body, Frame, Incoming},
    service::service_fn,
};
use hyper_util::rt::{TokioExecutor, TokioIo};

#[test]
fn request_signatures_use_fresh_nonces_and_the_shared_profile() {
    let key = SecretKey::from_bytes(&[7; 32]);
    let verifier = Verifier::new(
        key.public(),
        &key_id(&key.public()),
        "asimov",
        VerificationPolicy::request(),
    )
    .unwrap();
    let executor = Executor::new("https://example.com/api", Some("token"))
        .unwrap()
        .with_signing_key(key);
    let mut nonces = Vec::new();
    for _ in 0..2 {
        let mut request = executor
            .prepare_json_request("fetch", &json!({"input": "Українське"}))
            .unwrap();
        executor.sign_request(&mut request).unwrap();
        let message = Message::request(
            request.method().clone(),
            request.url().as_str(),
            request.headers().clone(),
        )
        .unwrap();
        let signature = verifier
            .verify(&message, SignatureParams::now().unwrap())
            .unwrap();
        nonces.push(String::from(signature.input.parameter("nonce").unwrap()));
        asimov_http::ContentDigest::new(request.body().unwrap().as_bytes().unwrap())
            .verify(request.headers(), &Default::default())
            .unwrap();
    }
    assert_ne!(nonces[0], nonces[1]);
    let executor = executor.with_digests(DigestOptions {
        content: false,
        ..Default::default()
    });
    assert!(matches!(
        executor.prepare_json_request("fetch", &json!({})),
        Err(Error::Integrity(_))
    ));
}

pub(super) struct Gated {
    first: bool,
    last: bool,
    gate: oneshot::Receiver<()>,
}

impl Gated {
    pub(super) fn new(gate: oneshot::Receiver<()>) -> Self {
        Self {
            first: false,
            last: false,
            gate,
        }
    }
}

impl Body for Gated {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();
        if !this.first {
            this.first = true;
            return Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(b"{}\n")))));
        }
        if !this.last {
            if Pin::new(&mut this.gate).poll(cx).is_pending() {
                return Poll::Pending;
            }
            this.last = true;
            return Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(b"[]\n")))));
        }
        Poll::Ready(None)
    }
}

#[tokio::test]
async fn reqwest_verifies_streamed_trailers_over_http1_and_http2() {
    for version in [Version::HTTP_11, Version::HTTP_2] {
        for (tamper, stall) in [(false, false), (true, false), (false, true)] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let client_key = SecretKey::from_bytes(&[1; 32]);
            let server_key = SecretKey::from_bytes(&[2; 32]);
            let components = vec![
                Component::new("@status").unwrap(),
                Component::new("content-type").unwrap(),
                Component::trailer("content-digest").unwrap(),
                Component::parse(r#""content-digest";req"#).unwrap(),
                Component::parse(r#""@target-uri";req"#).unwrap(),
            ];
            let verifier = Verifier::new(
                server_key.public(),
                &key_id(&server_key.public()),
                "s",
                VerificationPolicy::new(components.clone()),
            )
            .unwrap();
            let request_verifier = Verifier::new(
                client_key.public(),
                &key_id(&client_key.public()),
                "asimov",
                VerificationPolicy::request(),
            )
            .unwrap();
            let options = TimeoutOptions {
                body_idle: Some(if stall {
                    Duration::from_millis(200)
                } else {
                    Duration::from_secs(60)
                }),
                ..TimeoutOptions::default()
            };
            let mut executor = Executor::new_with_timeouts(
                format!("http://{address}/api"),
                Some("token"),
                options,
            )
            .unwrap()
            .with_signing_key(client_key)
            .with_response_verifier(verifier);
            if version == Version::HTTP_2 {
                executor.client = client_builder(options)
                    .unwrap()
                    .no_proxy()
                    .http2_prior_knowledge()
                    .build()
                    .unwrap();
            }
            let (resume, gate) = oneshot::channel();
            let gate = Arc::new(std::sync::Mutex::new(Some(gate)));
            let server = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                let service = service_fn(move |request: Request<Incoming>| {
                    let gate = gate.lock().unwrap().take().unwrap();
                    let verifier = request_verifier.clone();
                    let key = server_key.clone();
                    let components = components.clone();
                    async move {
                        assert_eq!(request.version(), version);
                        let (parts, body) = request.into_parts();
                        let request =
                            Request::from_parts(parts, body.collect().await.unwrap().to_bytes());
                        let target = format!("http://{address}/api/fetch");
                        verify_request(
                            &request,
                            &target,
                            &verifier,
                            SignatureParams::now().unwrap(),
                        )
                        .unwrap();
                        let context = Message::from_request(&request, &target).unwrap();
                        let input = SignatureInput::new(
                            components,
                            SignatureParams::fresh(&key.public(), "response-test").unwrap(),
                        )
                        .unwrap();
                        let signer = Signer::new(key, "s", input).unwrap();
                        let response = Response::builder()
                            .version(version)
                            .header("content-type", "application/jsonl")
                            .body(Gated {
                                first: false,
                                last: false,
                                gate,
                            })
                            .unwrap();
                        let response = SigningBody::response(
                            response,
                            Some(context),
                            Some(signer),
                            FieldSection::Trailers,
                        )
                        .unwrap();
                        Ok::<_, Infallible>(response.map(|body| {
                            body.map_frame(move |mut frame| {
                                if tamper {
                                    if let Some(trailers) = frame.trailers_mut() {
                                        trailers.insert(
                                            "content-digest",
                                            asimov_http::ContentDigest::new(b"altered")
                                                .to_field_value()
                                                .parse()
                                                .unwrap(),
                                        );
                                    }
                                }
                                frame
                            })
                        }))
                    }
                });
                if version == Version::HTTP_2 {
                    let _ = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                        .serve_connection(TokioIo::new(socket), service)
                        .await;
                } else {
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(socket), service)
                        .await;
                }
            });
            let mut stream = timeout(
                DEADLINE,
                executor.post_jsonl("fetch", &json!({"url": "example:"})),
            )
            .await
            .unwrap()
            .unwrap();
            let first = timeout(DEADLINE, stream.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(first.lines().collect::<Vec<_>>(), [b"{}\n".as_slice()]);
            if !stall {
                resume.send(()).unwrap();
            }
            let mut failed = false;
            let mut last = Vec::new();
            while let Some(batch) = timeout(DEADLINE, stream.next()).await.unwrap() {
                match batch {
                    Ok(batch) => {
                        for line in batch.lines() {
                            last.extend_from_slice(line);
                        }
                    },
                    Err(Error::Integrity(_)) if tamper => {
                        assert!(!failed);
                        failed = true;
                    },
                    Err(error)
                        if stall && error.timeout_phase() == Some(TimeoutPhase::BodyIdle) =>
                    {
                        assert!(!failed);
                        assert!(error.is_timeout());
                        failed = true;
                    },
                    Err(other) => panic!("unexpected error: {other}"),
                }
            }
            assert_eq!(
                last,
                if stall {
                    b"".as_slice()
                } else {
                    b"[]\n".as_slice()
                }
            );
            assert_eq!(failed, tamper || stall);
            server.abort();
            let _ = server.await;
        }
    }
}
