// This is free and unencumbered software released into the public domain.

use super::*;
use alloc::sync::Arc;
use asimov_flow::Bytes;
use asimov_http::body::sign_buffered_response;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use core::{
    future::Future,
    sync::atomic::{AtomicUsize, Ordering},
};
use http_body_util::{BodyExt as _, Full};
use hyper::{Request, Response, StatusCode, Version, body::Incoming, service::service_fn};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::Notify,
    task::{JoinHandle, JoinSet},
};
use tokio_rustls::TlsAcceptor;

const KEY_ID: &str = "deployment-key";

struct Fixture {
    executor: Executor,
    connections: Arc<AtomicUsize>,
    handshakes: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve<I, H, F>(io: I, version: Version, handler: Arc<H>)
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    H: Fn(Request<Bytes>) -> F + Send + Sync + 'static,
    F: Future<Output = Response<Bytes>> + Send,
{
    let service = service_fn(move |request: Request<Incoming>| {
        let handler = handler.clone();
        async move {
            assert_eq!(request.version(), version);
            let (parts, body) = request.into_parts();
            let request = Request::from_parts(parts, body.collect().await.unwrap().to_bytes());
            if request.method() == "GET" {
                assert_eq!(request.uri().path(), "/.well-known/jwks.json");
                assert!(request.uri().query().is_none());
                assert!(!request.headers().contains_key("authorization"));
                assert!(!request.headers().contains_key("signature"));
                assert!(request.body().is_empty());
            }
            Ok::<_, Infallible>(handler(request).await.map(Full::new))
        }
    });
    // A caller cancelling discovery can close its connection mid-response.
    if version == Version::HTTP_2 {
        let _ = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
            .serve_connection(TokioIo::new(io), service)
            .await;
    } else {
        let _ = hyper::server::conn::http1::Builder::new()
            .serve_connection(TokioIo::new(io), service)
            .await;
    }
}

async fn fixture<H, F>(version: Version, tls: bool, handler: H) -> Fixture
where
    H: Fn(Request<Bytes>) -> F + Send + Sync + 'static,
    F: Future<Output = Response<Bytes>> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none());
    let acceptor = if tls {
        let mut root_params = rcgen::CertificateParams::default();
        root_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        root_params.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign];
        root_params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "ASIMOV test CA");
        let root_key = rcgen::KeyPair::generate().unwrap();
        let root = root_params.self_signed(&root_key).unwrap();
        let issuer = rcgen::Issuer::new(root_params, root_key);
        let key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(vec!["localhost".into()]).unwrap();
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        let certificate = params.signed_by(&key, &issuer).unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .unwrap();
        config.alpn_protocols = vec![if version == Version::HTTP_2 {
            b"h2".to_vec()
        } else {
            b"http/1.1".to_vec()
        }];
        let mut config_client = tls_config().unwrap();
        let verifier = rustls_platform_verifier::Verifier::new_with_extra_roots(
            [root.der().clone()],
            config_client.crypto_provider().clone(),
        )
        .unwrap();
        config_client
            .dangerous()
            .set_certificate_verifier(Arc::new(verifier));
        client = client.tls_backend_preconfigured(config_client);
        Some(TlsAcceptor::from(Arc::new(config)))
    } else {
        if version == Version::HTTP_2 {
            client = client.http2_prior_knowledge();
        }
        None
    };
    if version == Version::HTTP_11 {
        client = client.http1_only();
    }
    let mut executor = Executor::new(
        format!(
            "{}://localhost:{}/api/relay?ignored#fragment",
            if tls { "https" } else { "http" },
            address.port()
        ),
        Some("token"),
    )
    .unwrap()
    .with_response_verification();
    executor.client = client.build().unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let handshakes = Arc::new(AtomicUsize::new(0));
    let accepted = connections.clone();
    let negotiated = handshakes.clone();
    let handler = Arc::new(handler);
    let task = tokio::spawn(async move {
        let mut tasks = JoinSet::new();
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            accepted.fetch_add(1, Ordering::SeqCst);
            let acceptor = acceptor.clone();
            let negotiated = negotiated.clone();
            let handler = handler.clone();
            tasks.spawn(async move {
                if let Some(acceptor) = acceptor {
                    let Ok(socket) = acceptor.accept(socket).await else {
                        return; // Certificate rejection closes the TLS handshake.
                    };
                    negotiated.fetch_add(1, Ordering::SeqCst);
                    serve(socket, version, handler).await;
                } else {
                    serve(socket, version, handler).await;
                }
            });
        }
    });
    Fixture {
        executor,
        connections,
        handshakes,
        task,
    }
}

fn jwk(key: &SecretKey) -> Value {
    json!({ "kty": "OKP", "crv": "Ed25519", "alg": "EdDSA", "use": "sig",
        "key_ops": ["verify"], "kid": KEY_ID,
        "x": URL_SAFE_NO_PAD.encode(key.public().as_bytes()) })
}

fn metadata(key: &SecretKey, policy: &str) -> Response<Bytes> {
    Response::builder()
        .header("content-type", "application/json")
        .header("cache-control", policy)
        .header("etag", "\"identity\"")
        .body(
            serde_json::to_vec(&json!({ "keys": [jwk(key)] }))
                .unwrap()
                .into(),
        )
        .unwrap()
}

fn signed_response(key: &SecretKey) -> Response<Bytes> {
    let mut params = SignatureParams::fresh(&key.public(), "remote-response").unwrap();
    params.key_id = KEY_ID.into();
    let signer = Signer::new(
        key.clone(),
        "asimov",
        SignatureInput::new(
            ["@status", "content-type", "content-digest"]
                .into_iter()
                .map(|name| Component::new(name).unwrap())
                .collect(),
            params,
        )
        .unwrap(),
    )
    .unwrap();
    let mut response = Response::builder()
        .header("content-type", "application/jsonl")
        .body(Bytes::from_static(b"{}\n"))
        .unwrap();
    sign_buffered_response(&mut response, None, &signer).unwrap();
    response
}

async fn consume(executor: &Executor, endpoint: &str) {
    let mut batches = timeout(DEADLINE, executor.post_jsonl(endpoint, &json!({})))
        .await
        .unwrap()
        .unwrap();
    let mut bytes = Vec::new();
    while let Some(batch) = timeout(DEADLINE, batches.next()).await.unwrap() {
        for line in batch.unwrap().lines() {
            bytes.extend_from_slice(line);
        }
    }
    assert_eq!(bytes, b"{}\n");
}

#[tokio::test]
async fn discovery_and_operations_reuse_http_and_tls_connections() {
    for version in [Version::HTTP_11, Version::HTTP_2] {
        for tls in [false, true] {
            let discoveries = Arc::new(AtomicUsize::new(0));
            let count = discoveries.clone();
            let fixture = fixture(version, tls, move |request| {
                let count = count.clone();
                async move {
                    let key = SecretKey::from_bytes(&[42; 32]);
                    if request.method() == "GET" {
                        count.fetch_add(1, Ordering::SeqCst);
                        metadata(&key, "max-age=300")
                    } else {
                        assert!(matches!(
                            request.uri().path(),
                            "/api/relay/fetch" | "/api/relay/list"
                        ));
                        assert_eq!(request.headers()["authorization"], "Bearer token");
                        if request.version() == Version::HTTP_11 {
                            assert_eq!(request.headers()["te"], "trailers");
                        }
                        assert!(request.headers().contains_key("signature"));
                        signed_response(&key)
                    }
                }
            })
            .await;
            let executor = fixture
                .executor
                .clone()
                .with_signing_key(SecretKey::from_bytes(&[7; 32]));
            consume(&executor, "fetch").await;
            consume(&executor.clone(), "list").await;
            assert_eq!(
                executor.remote_public_key().await.unwrap(),
                PublicKey::from(*SecretKey::from_bytes(&[42; 32]).public().as_bytes())
            );
            assert_eq!(discoveries.load(Ordering::SeqCst), 1);
            assert_eq!(fixture.connections.load(Ordering::SeqCst), 1);
            assert_eq!(fixture.handshakes.load(Ordering::SeqCst), usize::from(tls));
        }
    }
}

#[tokio::test]
async fn discovery_rate_limits_preserve_retry_after_and_allow_a_later_lookup() {
    let discoveries = Arc::new(AtomicUsize::new(0));
    let count = discoveries.clone();
    let fixture = fixture(Version::HTTP_11, false, move |request| {
        let count = count.clone();
        async move {
            let key = SecretKey::from_bytes(&[42; 32]);
            if request.method() == "GET" {
                if count.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Response::builder()
                        .status(429)
                        .header("retry-after", "0")
                        .body(Bytes::new())
                        .unwrap();
                }
                metadata(&key, "max-age=300")
            } else {
                signed_response(&key)
            }
        }
    })
    .await;
    let error = timeout(DEADLINE, fixture.executor.post_jsonl("fetch", &json!({})))
        .await
        .unwrap()
        .err()
        .unwrap();
    assert!(matches!(error, Error::RemoteKey(_)));
    assert_eq!(error.retry_after(), Some(Duration::ZERO));
    assert_eq!(discoveries.load(Ordering::SeqCst), 1);
    consume(&fixture.executor, "fetch").await;
    assert_eq!(discoveries.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn tls_rejects_untrusted_certificates_and_hostname_mismatches() {
    for trusted in [false, true] {
        let mut fixture = fixture(Version::HTTP_11, true, |_| async {
            panic!("an invalid TLS peer must not receive HTTP requests");
        })
        .await;
        if trusted {
            // The trusted leaf only identifies localhost, not its IP address.
            fixture
                .executor
                .base_url
                .set_host(Some("127.0.0.1"))
                .unwrap();
        } else {
            fixture.executor.client = Client::builder()
                .no_proxy()
                .tls_backend_preconfigured(tls_config().unwrap())
                .build()
                .unwrap();
        }
        let url = fixture
            .executor
            .base_url
            .join("/.well-known/jwks.json")
            .unwrap();
        let error = timeout(DEADLINE, fixture.executor.client.get(url).send())
            .await
            .unwrap()
            .unwrap_err();
        assert!(error.is_connect(), "{error:?}");
        assert_eq!(fixture.connections.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.handshakes.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn discovery_timeout_can_retry_without_using_an_incomplete_identity() {
    let discoveries = Arc::new(AtomicUsize::new(0));
    let count = discoveries.clone();
    let mut fixture = fixture(Version::HTTP_11, false, move |request| {
        let attempt = count.fetch_add(1, Ordering::SeqCst);
        async move {
            assert_eq!(request.method(), "GET");
            if attempt == 0 {
                core::future::pending::<()>().await;
            }
            metadata(&SecretKey::from_bytes(&[42; 32]), "max-age=300")
        }
    })
    .await;
    fixture.executor.timeouts.discovery = Some(Duration::from_millis(200));
    let error = timeout(DEADLINE, fixture.executor.remote_public_key())
        .await
        .unwrap()
        .unwrap_err();
    assert!(error.is_timeout());
    assert_eq!(error.timeout_phase(), Some(TimeoutPhase::Discovery));
    let key = timeout(DEADLINE, fixture.executor.remote_public_key())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        key,
        PublicKey::from(*SecretKey::from_bytes(&[42; 32]).public().as_bytes())
    );
    assert_eq!(fixture.executor.remote_public_key().await.unwrap(), key);
    assert_eq!(discoveries.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn total_budget_includes_discovery_and_headers_but_not_standalone_lookups() {
    let discoveries = Arc::new(AtomicUsize::new(0));
    let operations = Arc::new(AtomicUsize::new(0));
    let (gets, posts) = (discoveries.clone(), operations.clone());
    let mut fixture = fixture(Version::HTTP_11, false, move |request| {
        let (gets, posts) = (gets.clone(), posts.clone());
        async move {
            if request.method() == "POST" {
                posts.fetch_add(1, Ordering::SeqCst);
                core::future::pending::<()>().await;
            }
            if gets.fetch_add(1, Ordering::SeqCst) == 0 {
                core::future::pending::<()>().await;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
            metadata(&SecretKey::from_bytes(&[42; 32]), "max-age=300")
        }
    })
    .await;
    fixture.executor.timeouts.total = Some(Duration::from_millis(200));
    fixture.executor.timeouts.discovery = Some(Duration::from_secs(2));
    let error = timeout(DEADLINE, fixture.executor.post_jsonl("fetch", &json!({})))
        .await
        .unwrap()
        .err()
        .unwrap();
    assert!(error.is_timeout());
    assert_eq!(error.timeout_phase(), Some(TimeoutPhase::Total));
    assert_eq!(operations.load(Ordering::SeqCst), 0);
    timeout(DEADLINE, fixture.executor.remote_public_key())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(discoveries.load(Ordering::SeqCst), 2);
    let error = timeout(DEADLINE, fixture.executor.post_jsonl("fetch", &json!({})))
        .await
        .unwrap()
        .err()
        .unwrap();
    assert_eq!(error.timeout_phase(), Some(TimeoutPhase::Total));
    assert_eq!(operations.load(Ordering::SeqCst), 1);
    assert_eq!(discoveries.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn clones_share_in_flight_discovery() {
    let discoveries = Arc::new(AtomicUsize::new(0));
    let count = discoveries.clone();
    let received = Arc::new(Notify::new());
    let resume = Arc::new(Notify::new());
    let (notify, gate) = (received.clone(), resume.clone());
    let fixture = fixture(Version::HTTP_2, false, move |_| {
        let (count, notify, gate) = (count.clone(), notify.clone(), gate.clone());
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            notify.notify_one();
            gate.notified().await;
            metadata(&SecretKey::from_bytes(&[42; 32]), "max-age=300")
        }
    })
    .await;
    let mut tasks = JoinSet::new();
    for _ in 0..8 {
        let executor = fixture.executor.clone();
        tasks.spawn(async move { executor.remote_public_key().await.unwrap() });
    }
    timeout(DEADLINE, received.notified()).await.unwrap();
    resume.notify_one();
    while let Some(result) = timeout(DEADLINE, tasks.join_next()).await.unwrap() {
        assert_eq!(
            result.unwrap(),
            PublicKey::from(*SecretKey::from_bytes(&[42; 32]).public().as_bytes())
        );
    }
    assert_eq!(discoveries.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.connections.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn revalidates_expired_keys_and_replaces_rotated_identity() {
    let discoveries = Arc::new(AtomicUsize::new(0));
    let count = discoveries.clone();
    let fixture = fixture(Version::HTTP_11, false, move |request| {
        let count = count.clone();
        async move {
            if request.method() != "GET" {
                return signed_response(&SecretKey::from_bytes(&[43; 32]));
            }
            let attempt = count.fetch_add(1, Ordering::SeqCst);
            if attempt > 0 {
                assert_eq!(request.headers()["if-none-match"], "\"identity\"");
            }
            match attempt {
                0 => metadata(&SecretKey::from_bytes(&[42; 32]), "max-age=0"),
                // Omitted Cache-Control preserves the previous revalidation policy.
                1 => Response::builder().status(304).body(Bytes::new()).unwrap(),
                2 => metadata(&SecretKey::from_bytes(&[43; 32]), "max-age=300"),
                _ => panic!("fresh identity unexpectedly rediscovered"),
            }
        }
    })
    .await;
    for byte in [42, 42, 43, 43] {
        assert_eq!(
            fixture.executor.remote_public_key().await.unwrap(),
            PublicKey::from(*SecretKey::from_bytes(&[byte; 32]).public().as_bytes())
        );
    }
    consume(&fixture.executor, "fetch").await;
    assert_eq!(discoveries.load(Ordering::SeqCst), 3);
    assert_eq!(fixture.connections.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn respects_freshness_and_storage_directives() {
    for (policy, age, expected, conditional) in [
        ("max-age=60", "0", 1, false),
        ("max-age=60", "60", 2, true),
        ("no-cache, max-age=60", "0", 2, true),
        ("no-store, max-age=60", "0", 2, false),
        ("max-age=invalid", "0", 2, true),
        ("max-age=60, max-age=120", "0", 2, true),
    ] {
        let discoveries = Arc::new(AtomicUsize::new(0));
        let count = discoveries.clone();
        let fixture = fixture(Version::HTTP_11, false, move |request| {
            let count = count.clone();
            async move {
                if count.fetch_add(1, Ordering::SeqCst) > 0 {
                    assert_eq!(request.headers().contains_key("if-none-match"), conditional);
                }
                let mut response = metadata(&SecretKey::from_bytes(&[42; 32]), policy);
                response.headers_mut().insert("age", age.parse().unwrap());
                response
            }
        })
        .await;
        for _ in 0..2 {
            timeout(DEADLINE, fixture.executor.remote_public_key())
                .await
                .unwrap()
                .unwrap();
        }
        assert_eq!(
            discoveries.load(Ordering::SeqCst),
            expected,
            "{policy}, Age: {age}"
        );
    }
}

#[tokio::test]
async fn failed_discovery_never_uses_stale_keys_and_can_recover() {
    let key = SecretKey::from_bytes(&[42; 32]);
    let mut failures = vec![
        (StatusCode::NOT_FOUND, Bytes::new()),
        (
            StatusCode::FOUND,
            serde_json::to_vec(&json!({ "keys": [jwk(&key)] }))
                .unwrap()
                .into(),
        ),
        (StatusCode::OK, Bytes::from_static(b"{")),
        (StatusCode::OK, Bytes::from_static(br#"{"keys":[]}"#)),
        (
            StatusCode::OK,
            serde_json::to_vec(&json!({ "keys": [jwk(&key), jwk(&key)] }))
                .unwrap()
                .into(),
        ),
        (StatusCode::OK, Bytes::from(vec![b' '; 64 * 1024 + 1])),
    ];
    for (field, value) in [
        ("kty", json!("RSA")),
        ("crv", json!("X25519")),
        ("alg", json!("RS256")),
        ("use", json!("enc")),
        ("key_ops", json!(["sign"])),
        ("x", json!("invalid!")),
        ("x", json!("AA")),
        ("x", Value::Null),
        ("x", json!(URL_SAFE_NO_PAD.encode([2; 32]))),
    ] {
        let mut key = jwk(&key);
        key[field] = value;
        failures.push((
            StatusCode::OK,
            serde_json::to_vec(&json!({ "keys": [key] }))
                .unwrap()
                .into(),
        ));
    }
    for (status, body) in failures {
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let fixture = fixture(Version::HTTP_11, false, move |request| {
            let (count, body) = (count.clone(), body.clone());
            async move {
                match count.fetch_add(1, Ordering::SeqCst) {
                    0 => metadata(&SecretKey::from_bytes(&[42; 32]), "max-age=0"),
                    1 => {
                        assert_eq!(request.method(), "GET");
                        Response::builder()
                            .status(status)
                            .header("location", "/unexpected")
                            .body(body)
                            .unwrap()
                    },
                    2 => {
                        assert_eq!(request.method(), "GET");
                        metadata(&SecretKey::from_bytes(&[43; 32]), "max-age=300")
                    },
                    3 => {
                        assert_eq!(request.method(), "POST");
                        signed_response(&SecretKey::from_bytes(&[43; 32]))
                    },
                    _ => panic!("unexpected request"),
                }
            }
        })
        .await;
        fixture.executor.remote_public_key().await.unwrap();
        assert!(matches!(
            timeout(DEADLINE, fixture.executor.post_jsonl("fetch", &json!({})))
                .await
                .unwrap(),
            Err(Error::RemoteKey(_))
        ));
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        consume(&fixture.executor, "fetch").await;
        assert_eq!(requests.load(Ordering::SeqCst), 4);
    }
}

#[tokio::test]
async fn explicit_verifier_replaces_automatic_discovery() {
    let key = SecretKey::from_bytes(&[42; 32]);
    let fixture = fixture(Version::HTTP_11, false, move |request| async move {
        assert_eq!(request.method(), "POST");
        signed_response(&SecretKey::from_bytes(&[42; 32]))
    })
    .await;
    let verifier = Verifier::new(
        key.public(),
        KEY_ID,
        "asimov",
        VerificationPolicy::new(vec![
            Component::new("@status").unwrap(),
            Component::new("content-type").unwrap(),
        ]),
    )
    .unwrap();
    consume(
        &fixture.executor.clone().with_response_verifier(verifier),
        "fetch",
    )
    .await;
}

#[tokio::test]
async fn cancelled_lookup_does_not_poison_shared_discovery() {
    let received = Arc::new(Notify::new());
    let notify = received.clone();
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    let fixture = fixture(Version::HTTP_2, false, move |_| {
        let (count, notify) = (count.clone(), notify.clone());
        async move {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                notify.notify_one();
                core::future::pending::<()>().await;
            }
            metadata(&SecretKey::from_bytes(&[42; 32]), "max-age=300")
        }
    })
    .await;
    let executor = fixture.executor.clone();
    let task = tokio::spawn(async move { executor.remote_public_key().await });
    timeout(DEADLINE, received.notified()).await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    timeout(DEADLINE, fixture.executor.remote_public_key())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.load(Ordering::SeqCst), 2);
}
