// This is free and unencumbered software released into the public domain.

use super::remote_identity::{KEY_ID, fixture, metadata};
use super::*;
use alloc::string::ToString;
use asimov_flow::Bytes;
use asimov_http::{
    body::{sign_buffered_response, verify_request},
    key_id,
};
use hyper::{Request, Response, StatusCode, Version};
use tokio::time::Instant;

fn response(key: &SecretKey, status: StatusCode, body: &'static [u8]) -> Response<Bytes> {
    let mut params = SignatureParams::fresh(&key.public(), "remote-check").unwrap();
    params.key_id = KEY_ID.into();
    let signer = Signer::new(
        key.clone(),
        "asimov",
        SignatureInput::new(
            ["@status", "content-digest"]
                .into_iter()
                .map(|name| Component::new(name).unwrap())
                .collect(),
            params,
        )
        .unwrap(),
    )
    .unwrap();
    let mut response = Response::builder()
        .status(status)
        .body(Bytes::from_static(body))
        .unwrap();
    sign_buffered_response(&mut response, None, &signer).unwrap();
    response
}

fn verify_options(request: &Request<Bytes>) {
    assert_eq!(request.method(), "OPTIONS");
    assert_eq!(
        request.uri().path_and_query().unwrap(),
        "/api/relay?ignored"
    );
    assert!(request.body().is_empty());
    assert_eq!(request.headers()["authorization"], "Bearer token");
    let key = SecretKey::from_bytes(&[1; 32]).public();
    let verifier =
        Verifier::new(key, &key_id(&key), "asimov", VerificationPolicy::request()).unwrap();
    let target = if request.version() == Version::HTTP_2 {
        request.uri().to_string()
    } else {
        format!(
            "http://{}{}",
            request.headers()["host"].to_str().unwrap(),
            request.uri()
        )
    };
    verify_request(request, &target, &verifier, SignatureParams::now().unwrap()).unwrap();
}

#[tokio::test]
async fn check_signs_the_exact_target_and_discovers_the_remote_signing_key() {
    for version in [Version::HTTP_11, Version::HTTP_2] {
        for (seed, status, body) in [
            (42, StatusCode::NO_CONTENT, b"".as_slice()),
            (
                53,
                StatusCode::OK,
                b"\xffOPTIONS response, not JSONL".as_slice(),
            ),
        ] {
            let fixture = fixture(version, false, move |request| async move {
                let key = SecretKey::from_bytes(&[seed; 32]);
                if request.method() == "GET" {
                    metadata(&key, "max-age=300")
                } else {
                    verify_options(&request);
                    response(&key, status, body)
                }
            })
            .await;
            let mut executor = fixture
                .executor
                .clone()
                .with_signing_key(SecretKey::from_bytes(&[1; 32]));
            // Checks always verify, even without enabling fetch/list verification.
            executor.response_verification = ResponseVerification::Disabled;
            let rtt = timeout(DEADLINE, executor.check()).await.unwrap().unwrap();
            assert!(!rtt.is_zero());
        }
    }
}

#[tokio::test]
async fn check_rejects_unsigned_tampered_expired_and_wrong_key_responses() {
    for failure in [
        "unsigned",
        "body",
        "key",
        "expired",
        "status",
        "digest-coverage",
    ] {
        let fixture = fixture(Version::HTTP_11, false, move |request| async move {
            let key = SecretKey::from_bytes(&[42; 32]);
            if request.method() == "GET" {
                return metadata(&key, "max-age=300");
            }
            let mut result = response(&key, StatusCode::OK, b"original");
            match failure {
                "unsigned" => {
                    result.headers_mut().remove("signature");
                },
                "body" => *result.body_mut() = Bytes::from_static(b"modified"),
                "key" => {
                    result = response(
                        &SecretKey::from_bytes(&[99; 32]),
                        StatusCode::OK,
                        b"original",
                    )
                },
                "status" => *result.status_mut() = StatusCode::CREATED,
                "expired" | "digest-coverage" => {
                    let mut params = SignatureParams::fresh(&key.public(), "remote-check").unwrap();
                    params.key_id = KEY_ID.into();
                    if failure == "expired" {
                        params.created -= 300;
                        params.expires = params.expires.map(|expires| expires - 300);
                    }
                    let components = if failure == "expired" {
                        vec![
                            Component::new("@status").unwrap(),
                            Component::new("content-digest").unwrap(),
                        ]
                    } else {
                        vec![Component::new("@status").unwrap()]
                    };
                    let signer = Signer::new(
                        key,
                        "asimov",
                        SignatureInput::new(components, params).unwrap(),
                    )
                    .unwrap();
                    result.headers_mut().remove("signature");
                    result.headers_mut().remove("signature-input");
                    let message =
                        Message::response(result.status(), result.headers().clone(), None).unwrap();
                    signer
                        .sign(&message)
                        .unwrap()
                        .append_to(result.headers_mut())
                        .unwrap();
                },
                _ => unreachable!(),
            }
            result
        })
        .await;
        let executor = fixture
            .executor
            .clone()
            .with_signing_key(SecretKey::from_bytes(&[1; 32]));
        assert!(
            matches!(
                timeout(DEADLINE, executor.check()).await.unwrap(),
                Err(Error::Integrity(_))
            ),
            "{failure}"
        );
    }
}

#[tokio::test]
async fn check_rejects_http_errors_and_redirects_without_retrying() {
    for status in [
        StatusCode::METHOD_NOT_ALLOWED,
        StatusCode::TEMPORARY_REDIRECT,
        StatusCode::TOO_MANY_REQUESTS,
    ] {
        let fixture = fixture(Version::HTTP_11, false, move |request| async move {
            let key = SecretKey::from_bytes(&[42; 32]);
            if request.method() == "GET" {
                return metadata(&key, "max-age=300");
            }
            verify_options(&request);
            let mut result = response(&key, status, b"");
            result
                .headers_mut()
                .insert("location", "/must-not-follow".parse().unwrap());
            result
                .headers_mut()
                .insert("retry-after", "1".parse().unwrap());
            result
        })
        .await;
        let executor = fixture
            .executor
            .clone()
            .with_signing_key(SecretKey::from_bytes(&[1; 32]));
        let error = timeout(DEADLINE, executor.check())
            .await
            .unwrap()
            .unwrap_err();
        match status {
            StatusCode::METHOD_NOT_ALLOWED => {
                assert!(matches!(error, Error::Http(error) if error.status() == Some(status)))
            },
            StatusCode::TEMPORARY_REDIRECT => {
                assert!(matches!(error, Error::Redirect(found) if found == status))
            },
            _ => assert_eq!(error.retry_after(), Some(Duration::from_secs(1))),
        }
    }
}

#[tokio::test]
async fn check_requires_signing_and_a_discoverable_identity() {
    let executor = Executor::new("http://127.0.0.1:1", Some("token")).unwrap();
    assert!(matches!(executor.check().await, Err(Error::Integrity(_))));
    let executor = executor
        .with_signing_key(SecretKey::from_bytes(&[1; 32]))
        .with_digests(DigestOptions {
            content: false,
            ..Default::default()
        });
    assert!(matches!(executor.check().await, Err(Error::Integrity(_))));
    let fixture = fixture(Version::HTTP_11, false, |request| async move {
        assert_eq!(request.method(), "GET");
        Response::builder()
            .body(Bytes::from_static(br#"{"keys":[]}"#))
            .unwrap()
    })
    .await;
    let executor = fixture
        .executor
        .clone()
        .with_signing_key(SecretKey::from_bytes(&[1; 32]));
    assert!(matches!(
        timeout(DEADLINE, executor.check()).await.unwrap(),
        Err(Error::RemoteKey(_))
    ));
}

#[tokio::test]
async fn check_rtt_excludes_discovery_and_header_waits_are_bounded() {
    let discovery = Arc::new(std::sync::Mutex::new(Duration::ZERO));
    let measured = discovery.clone();
    let fixture = fixture(Version::HTTP_11, false, move |request| {
        let measured = measured.clone();
        async move {
            let key = SecretKey::from_bytes(&[42; 32]);
            if request.method() == "GET" {
                let started = Instant::now();
                tokio::time::sleep(Duration::from_millis(100)).await;
                *measured.lock().unwrap() = started.elapsed();
                metadata(&key, "max-age=300")
            } else {
                tokio::time::sleep(Duration::from_millis(50)).await;
                response(&key, StatusCode::NO_CONTENT, b"")
            }
        }
    })
    .await;
    let mut executor = fixture
        .executor
        .clone()
        .with_signing_key(SecretKey::from_bytes(&[1; 32]));
    let started = Instant::now();
    let rtt = timeout(DEADLINE, executor.check()).await.unwrap().unwrap();
    assert!(rtt >= Duration::from_millis(50));
    assert!(started.elapsed() >= rtt + *discovery.lock().unwrap());
    executor.timeouts.response_headers = Some(Duration::from_millis(10));
    let error = timeout(DEADLINE, executor.check())
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.timeout_phase(), Some(TimeoutPhase::ResponseHeaders));
}

#[tokio::test]
async fn check_waits_for_verified_trailers_and_bounds_body_completion() {
    use asimov_http::{FieldSection, body::SigningBody};
    use http_body_util::BodyExt as _;
    use hyper::{body::Incoming, service::service_fn};
    use hyper_util::rt::{TokioExecutor, TokioIo};

    for version in [Version::HTTP_11, Version::HTTP_2] {
        for mode in ["valid", "tampered", "idle", "total"] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let key = SecretKey::from_bytes(&[42; 32]);
            let verifier = Verifier::new(
                key.public(),
                &key_id(&key.public()),
                "asimov",
                VerificationPolicy::new(vec![Component::new("@status").unwrap()]),
            )
            .unwrap();
            let options = TimeoutOptions {
                body_idle: Some(if mode == "idle" {
                    Duration::from_millis(200)
                } else {
                    DEADLINE
                }),
                total: (mode == "total").then_some(Duration::from_millis(200)),
                ..TimeoutOptions::default()
            };
            let mut executor = Executor::new_with_timeouts(
                format!("http://{address}/api"),
                Some("token"),
                options,
            )
            .unwrap()
            .with_signing_key(SecretKey::from_bytes(&[1; 32]))
            .with_response_verifier(verifier);
            let client = client_builder(options).unwrap().no_proxy();
            executor.client = if version == Version::HTTP_2 {
                client.http2_prior_knowledge()
            } else {
                client
            }
            .build()
            .unwrap();
            let (resume, gate) = oneshot::channel();
            let gate = Arc::new(std::sync::Mutex::new(Some(gate)));
            let (sent, headers) = oneshot::channel();
            let sent = Arc::new(std::sync::Mutex::new(Some(sent)));
            let server = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                let service = service_fn(move |request: Request<Incoming>| {
                    let gate = gate.lock().unwrap().take().unwrap();
                    let sent = sent.lock().unwrap().take().unwrap();
                    let key = key.clone();
                    async move {
                        assert_eq!(request.method(), "OPTIONS");
                        let context =
                            Message::from_request(&request, &format!("http://{address}/api"))
                                .unwrap();
                        request.into_body().collect().await.unwrap();
                        let input = SignatureInput::new(
                            vec![
                                Component::new("@status").unwrap(),
                                Component::trailer("content-digest").unwrap(),
                                Component::parse(r#""@target-uri";req"#).unwrap(),
                                Component::parse(r#""signature";req"#).unwrap(),
                            ],
                            SignatureParams::fresh(&key.public(), "remote-check").unwrap(),
                        )
                        .unwrap();
                        let signer = Signer::new(key, "asimov", input).unwrap();
                        let response = Response::builder()
                            .version(version)
                            .body(super::signature::Gated::new(gate))
                            .unwrap();
                        let response = SigningBody::response(
                            response,
                            Some(context),
                            Some(signer),
                            FieldSection::Trailers,
                        )
                        .unwrap();
                        sent.send(()).unwrap();
                        Ok::<_, Infallible>(response.map(|body| {
                            body.map_frame(move |mut frame| {
                                if mode == "tampered"
                                    && let Some(trailers) = frame.trailers_mut()
                                {
                                    trailers.remove("signature");
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
            let mut check = tokio::spawn(async move { executor.check().await });
            timeout(DEADLINE, headers).await.unwrap().unwrap();
            assert!(
                timeout(Duration::from_millis(20), &mut check)
                    .await
                    .is_err()
            );
            if matches!(mode, "valid" | "tampered") {
                resume.send(()).unwrap();
            }
            let result = timeout(DEADLINE, check).await.unwrap().unwrap();
            match mode {
                "valid" => assert!(result.unwrap() >= Duration::from_millis(20)),
                "tampered" => assert!(matches!(result, Err(Error::Integrity(_)))),
                "idle" => assert_eq!(
                    result.unwrap_err().timeout_phase(),
                    Some(TimeoutPhase::BodyIdle)
                ),
                _ => assert_eq!(
                    result.unwrap_err().timeout_phase(),
                    Some(TimeoutPhase::Total)
                ),
            }
            server.abort();
            let _ = server.await;
        }
    }
}
