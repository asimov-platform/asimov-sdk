// This is free and unencumbered software released into the public domain.

use super::*;
use crate::http::{Executor, StreamExt, TimeoutOptions, client_builder, tls_config};
use alloc::{string::ToString as _, sync::Arc};
use core::{convert::Infallible, time::Duration};
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, body::Bytes, service::service_fn};
use hyper_util::rt::TokioIo;
use reqwest::{ClientBuilder, Proxy};
use system_configuration::core_foundation::{array::CFArray, boolean::CFBoolean, number::CFNumber};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};

const DEADLINE: Duration = Duration::from_secs(5);

fn settings() -> Dictionary {
    Dictionary::from_iter([
        (String::from("HTTPEnable"), Value::Integer(1.into())),
        (
            String::from("HTTPProxy"),
            Value::String("http.proxy.invalid".into()),
        ),
        (String::from("HTTPPort"), Value::Integer(8080.into())),
        (String::from("HTTPSEnable"), Value::Boolean(true)),
        (
            String::from("HTTPSProxy"),
            Value::String("https.proxy.invalid".into()),
        ),
        (String::from("HTTPSPort"), Value::Integer(8443.into())),
    ])
}

fn exceptions(settings: &mut Dictionary, entries: &[&str]) {
    settings.insert(
        "ExceptionsList".into(),
        Value::Array(entries.iter().map(|s| Value::String((*s).into())).collect()),
    );
}

fn route(url: &str, settings: Dictionary, env: &Environment) -> Result<Route, Error> {
    select(&Url::parse(url).unwrap(), env, || Ok(Some(settings)))
}

fn configure_with(
    builder: ClientBuilder,
    url: &Url,
    env: &Environment,
    read: impl FnOnce() -> Result<Option<Dictionary>, Error>,
) -> Result<ClientBuilder, Error> {
    crate::http::proxy::configure_with(builder, env, || select_native(url, env, read))
}

#[test]
fn snapshots_heterogeneous_native_values_without_typed_array_casts() {
    let exceptions = CFArray::from_CFTypes(&[
        CFString::new("*.corp.invalid").as_CFType(),
        CFNumber::from(42).as_CFType(),
        CFString::new("169.254/16").as_CFType(),
    ]);
    let native = CFDictionary::from_CFType_pairs(&[
        (
            CFString::new("HTTPEnable"),
            CFBoolean::true_value().as_CFType(),
        ),
        (
            CFString::new("HTTPProxy"),
            CFString::new("proxy.invalid").as_CFType(),
        ),
        (CFString::new("HTTPPort"), CFNumber::from(3128).as_CFType()),
        (
            CFString::new("ExcludeSimpleHostnames"),
            CFNumber::from(0).as_CFType(),
        ),
        (CFString::new("ExceptionsList"), exceptions.as_CFType()),
    ]);
    let config = snapshot(&native).unwrap();
    let env = Environment::default();
    assert_eq!(
        route("http://a.corp.invalid", config.clone(), &env).unwrap(),
        Route::Direct
    );
    assert_eq!(
        route("http://169.254.1.2", config.clone(), &env).unwrap(),
        Route::Direct
    );
    assert_eq!(
        route("http://corp.invalid", config.clone(), &env).unwrap(),
        Route::Native(Url::parse("http://proxy.invalid:3128").unwrap())
    );
    assert!(matches!(
        route("http://intranet", config, &env).unwrap(),
        Route::Native(_)
    ));
}

#[test]
fn selects_protocol_specific_http_proxies_and_validates_only_the_selected_one() {
    let env = Environment::default();
    assert_eq!(
        route("http://service.invalid", settings(), &env).unwrap(),
        Route::Native(Url::parse("http://http.proxy.invalid:8080").unwrap())
    );
    assert_eq!(
        route("https://service.invalid", settings(), &env).unwrap(),
        Route::Native(Url::parse("http://https.proxy.invalid:8443").unwrap())
    );
    let mut config = settings();
    config.remove("HTTPPort");
    config.insert("HTTPSPort".into(), Value::Integer((-1).into()));
    assert_eq!(
        route("http://service.invalid", config.clone(), &env).unwrap(),
        Route::Native(Url::parse("http://http.proxy.invalid").unwrap())
    );
    config.insert("HTTPProxy".into(), Value::String("2001:db8::1".into()));
    assert_eq!(
        route("http://service.invalid", config.clone(), &env).unwrap(),
        Route::Native(Url::parse("http://[2001:db8::1]").unwrap())
    );
    config.insert("HTTPEnable".into(), Value::Boolean(false));
    assert_eq!(
        route("http://service.invalid", config, &env).unwrap(),
        Route::Automatic
    );
    assert_eq!(
        select(&Url::parse("http://service.invalid").unwrap(), &env, || Ok(
            None
        ))
        .unwrap(),
        Route::Automatic
    );
}

#[test]
fn malformed_native_endpoints_report_setting_names_without_credentials() {
    for host in [
        "",
        " bad.invalid",
        "bad.invalid ",
        "https://bad.invalid",
        "User:Secret@bad.invalid",
        "bad.invalid/path",
        "bad.invalid:8080",
        "bad%2einvalid",
    ] {
        let mut config = settings();
        config.insert("HTTPProxy".into(), Value::String(host.into()));
        let error = route("http://service.invalid", config, &Environment::default()).unwrap_err();
        assert!(matches!(error, Error::InvalidProxy("HTTPProxy")));
        assert!(!error.to_string().contains("Secret"));
    }
    for port in [
        Value::Integer((-1).into()),
        Value::Integer(0.into()),
        Value::Integer(65536.into()),
        Value::Real(3128.5),
        Value::String("3128".into()),
    ] {
        let mut config = settings();
        config.insert("HTTPPort".into(), port);
        assert!(matches!(
            route("http://service.invalid", config, &Environment::default()),
            Err(Error::InvalidProxy("HTTPPort"))
        ));
    }
}

#[test]
fn native_socks_fills_empty_protocol_slots_with_remote_dns() {
    let env = Environment::default();
    let mut config = settings();
    config.insert("SOCKSEnable".into(), Value::Integer(1.into()));
    config.insert("SOCKSProxy".into(), Value::String("socks.invalid".into()));
    config.insert("SOCKSPort".into(), Value::Integer(1081.into()));
    assert_eq!(
        route("http://service.invalid", config.clone(), &env).unwrap(),
        Route::Native(Url::parse("http://http.proxy.invalid:8080").unwrap())
    );
    config.insert("HTTPSEnable".into(), Value::Boolean(false));
    assert_eq!(
        route("https://service.invalid", config.clone(), &env).unwrap(),
        Route::Native(Url::parse("socks5h://socks.invalid:1081/").unwrap())
    );
    config.remove("SOCKSPort");
    assert_eq!(
        route("https://service.invalid", config.clone(), &env).unwrap(),
        Route::Native(Url::parse("socks5h://socks.invalid:1080/").unwrap())
    );
    exceptions(&mut config, &["*.corp.invalid"]);
    assert_eq!(
        route("https://a.corp.invalid", config.clone(), &env).unwrap(),
        Route::Direct
    );
    config.insert("SOCKSPort".into(), Value::Integer(0.into()));
    assert!(matches!(
        route("https://service.invalid", config.clone(), &env),
        Err(Error::InvalidProxy("SOCKSPort"))
    ));
    // A broken, unselected SOCKS proxy cannot break the HTTP slot.
    assert!(matches!(
        route("http://service.invalid", config, &env),
        Ok(Route::Native(_))
    ));
}

#[test]
fn native_exceptions_use_label_patterns_and_literal_addresses() {
    // These hostname/label cases were checked with CFNetworkCopyProxiesForURL
    // on macOS, using synthetic dictionaries rather than changing OS settings.
    for (rule, host, expected) in [
        ("corp.invalid", "corp.invalid", true),
        ("CORP.INVALID", "corp.invalid", true),
        ("corp.invalid", "a.corp.invalid", false),
        ("corp.invalid", "notcorp.invalid", false),
        ("corp.invalid.", "corp.invalid", true),
        ("corp.invalid", "corp.invalid.", true),
        ("*.corp.invalid", "corp.invalid", false),
        ("*.corp.invalid", "a.corp.invalid", true),
        ("*.corp.invalid", "b.a.corp.invalid", true),
        (".corp.invalid", "a.corp.invalid.", true),
        ("corp.*", "corp", true),
        ("corp.*", "corp.other.invalid", true),
        ("corp.*", "other.corp.invalid", false),
        ("*.corp.*", "a.corp.invalid", true),
        ("*.corp.*", "corp.invalid", false),
        ("a.*.invalid", "a.corp.invalid", true),
        ("foo*", "foobar", false),
        ("*corp.invalid", "foocorp.invalid", false),
        ("*", "corp.invalid", false),
        ("<local>", "intranet", false),
        ("corp.invalid:80", "corp.invalid", false),
        (" corp.invalid", "corp.invalid", false),
        ("corp.invalid ", "corp.invalid", false),
        ("corp.invalid..", "corp.invalid", false),
        ("192.168.*.*", "192.168.1.2", true),
        ("192.168.*", "192.169.1.2", false),
        ("169.254/16", "169.254.1.2", true),
        ("169.254/16", "169.253.1.2", false),
        ("10.0.0.0/8", "10.2.3.4", true),
        ("10.0.0.0/8", "service.invalid", false),
        ("2001:db8::1", "2001:db8::1", true),
        ("[2001:db8::1]", "2001:db8::1", false),
        ("2001:db8::/32", "2001:db8::1", false),
        ("bücher.invalid", "xn--bcher-kva.invalid", false),
        ("xn--bcher-kva.invalid", "xn--bcher-kva.invalid", true),
    ] {
        assert_eq!(matches(rule, host), expected, "{rule:?} against {host:?}");
    }
    // Use conventional CIDR boundaries rather than CFNetwork's shift edge cases.
    assert!(matches("0.0.0.0/0", "192.0.2.1"));
    assert!(matches("192.0.2.1/32", "192.0.2.1"));
    assert!(!matches("192.0.2.1/32", "192.0.2.2"));
    for rule in [
        "10/33",
        "10/-1",
        "256/8",
        "10..1/8",
        "10.0.0.0.1/8",
        "/8",
        "+10/8",
        "010/8",
        "10/+8",
    ] {
        assert!(!matches(rule, "10.0.0.1"), "{rule}");
    }
}

#[test]
fn simple_hostnames_require_enablement_and_never_include_ip_literals() {
    for (value, enabled) in [
        (Value::Boolean(false), false),
        (Value::Boolean(true), true),
        (Value::Integer(0.into()), false),
        (Value::Integer(1.into()), true),
    ] {
        let mut config = settings();
        config.insert("ExcludeSimpleHostnames".into(), value);
        assert_eq!(
            bypasses(&config, &Url::parse("http://intranet").unwrap()),
            enabled
        );
        for host in [
            "intranet.",
            "service.invalid",
            "192.0.2.1",
            "[2001:db8::1]",
            "[::ffff:127.0.0.1]",
        ] {
            assert!(
                !bypasses(&config, &Url::parse(&format!("http://{host}")).unwrap()),
                "{host}"
            );
        }
    }
}

#[test]
fn native_loopback_exclusions_require_a_bypass_setting() {
    let mut config = settings();
    for host in ["localhost", "127.0.0.1", "[::1]"] {
        assert!(!bypasses(
            &config,
            &Url::parse(&format!("http://{host}")).unwrap()
        ));
    }
    exceptions(&mut config, &[]);
    for host in ["localhost", "LOCALHOST", "127.0.0.1", "[::1]"] {
        assert!(bypasses(
            &config,
            &Url::parse(&format!("http://{host}")).unwrap()
        ));
    }
    for host in [
        "localhost.",
        "a.localhost",
        "127.0.0.2",
        "[::ffff:127.0.0.1]",
    ] {
        assert!(!bypasses(
            &config,
            &Url::parse(&format!("http://{host}")).unwrap()
        ));
    }
}

#[test]
fn environment_and_cgi_take_precedence_without_reading_native_settings() {
    let url = Url::parse("https://service.invalid").unwrap();
    for env in [
        Environment {
            proxy: "socks5h://User:Secret@proxy.invalid:1080".into(),
            ..Environment::default()
        },
        Environment {
            cgi: true,
            ..Environment::default()
        },
    ] {
        assert_eq!(
            select(&url, &env, || panic!("native settings must not be read")).unwrap(),
            if env.cgi {
                Route::Direct
            } else {
                Route::Automatic
            }
        );
    }
    let mut config = settings();
    exceptions(&mut config, &["service.invalid"]);
    for env in [
        Environment {
            no_proxy: "other.invalid".into(),
            ..Environment::default()
        },
        Environment {
            no_proxy_override: true,
            ..Environment::default()
        },
    ] {
        assert!(matches!(
            select(&url, &env, || Ok(Some(config.clone()))).unwrap(),
            Route::Native(_)
        ));
    }
}

struct Peer {
    address: core::net::SocketAddr,
    requests: tokio::sync::mpsc::UnboundedReceiver<Request<Bytes>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(
    io: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
    requests: tokio::sync::mpsc::UnboundedSender<Request<Bytes>>,
) {
    let service = service_fn(move |request: Request<hyper::body::Incoming>| {
        let requests = requests.clone();
        async move {
            let identity = request.uri().path() == "/.well-known/jwks.json";
            let (parts, body) = request.into_parts();
            let body = body.collect().await.unwrap().to_bytes();
            requests.send(Request::from_parts(parts, body)).unwrap();
            let body = if identity {
                // RFC 8037's Ed25519 public key, without private key material.
                Bytes::from_static(br#"{"keys":[{"kty":"OKP","crv":"Ed25519","x":"11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"}]}"#)
            } else {
                Bytes::from_static(b"{}\n")
            };
            Ok::<_, Infallible>(Response::new(Full::new(body)))
        }
    });
    let _ = hyper::server::conn::http1::Builder::new()
        .serve_connection(TokioIo::new(io), service)
        .await;
}

async fn peer() -> Peer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (requests, received) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            serve(socket, requests.clone()).await;
        }
    });
    Peer {
        address,
        requests: received,
        task,
    }
}

async fn exercise(executor: &Executor, peer: &mut Peer, proxied: bool) {
    timeout(DEADLINE, executor.remote_public_key())
        .await
        .unwrap()
        .unwrap();
    let mut stream = timeout(
        DEADLINE,
        executor.post_jsonl("fetch", &serde_json::json!({})),
    )
    .await
    .unwrap()
    .unwrap();
    while let Some(batch) = timeout(DEADLINE, stream.next()).await.unwrap() {
        batch.unwrap();
    }
    for path in ["/.well-known/jwks.json", "/api/fetch"] {
        let request = timeout(DEADLINE, peer.requests.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request.uri().path(), path);
        assert_eq!(request.uri().scheme().is_some(), proxied);
        assert!(!request.headers().contains_key("proxy-authorization"));
        if path.ends_with("fetch") {
            assert_eq!(request.headers()["authorization"], "Bearer service-token");
            assert!(request.headers().contains_key("content-digest"));
        } else {
            assert!(!request.headers().contains_key("authorization"));
        }
    }
}

#[tokio::test]
async fn native_proxy_and_bypass_route_both_discovery_and_execution() {
    for (native_bypass, no_proxy, proxied) in [
        (None, "", true),
        (Some("service.invalid"), "", false),
        (Some("service.invalid"), "other.invalid", true),
        (None, "service.invalid", false),
        (Some("127.0.0.0/8"), "", true),
    ] {
        let mut origin = peer().await;
        let mut proxy = peer().await;
        let url = Url::parse(&format!(
            "http://service.invalid:{}/api/",
            origin.address.port()
        ))
        .unwrap();
        let mut config = settings();
        config.insert("HTTPProxy".into(), Value::String("127.0.0.1".into()));
        config.insert(
            "HTTPPort".into(),
            Value::Integer(proxy.address.port().into()),
        );
        if let Some(rule) = native_bypass {
            exceptions(&mut config, &[rule]);
        }
        let env = Environment {
            no_proxy: no_proxy.into(),
            ..Environment::default()
        };
        let mut executor = Executor::new(url.as_str(), Some("service-token")).unwrap();
        let mut builder = client_builder(TimeoutOptions::default())
            .unwrap()
            .no_proxy()
            .resolve("service.invalid", origin.address);
        if native_bypass.is_some() && no_proxy.is_empty() && !proxied {
            // A native bypass must clear fallback proxy selection, not merely
            // decline to install the native proxy on an already-direct client.
            builder = builder.proxy(Proxy::all(format!("http://{}", proxy.address)).unwrap());
        }
        executor.client = configure_with(builder, &url, &env, || Ok(Some(config)))
            .unwrap()
            .build()
            .unwrap();
        if proxied {
            exercise(&executor, &mut proxy, true).await;
            assert!(origin.requests.try_recv().is_err());
        } else {
            exercise(&executor, &mut origin, false).await;
            assert!(proxy.requests.try_recv().is_err());
        }
    }
}

#[tokio::test]
async fn native_https_uses_connect_and_platform_verified_origin_tls() {
    let mut root_params = rcgen::CertificateParams::default();
    root_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    root_params.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign];
    let root_key = rcgen::KeyPair::generate().unwrap();
    let root = root_params.self_signed(&root_key).unwrap();
    let issuer = rcgen::Issuer::new(root_params, root_key);
    let key = rcgen::KeyPair::generate().unwrap();
    let mut params = rcgen::CertificateParams::new(vec!["service.invalid".into()]).unwrap();
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    let certificate = params.signed_by(&key, &issuer).unwrap();
    let server = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![certificate.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
    )
    .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (requests, received) = tokio::sync::mpsc::unbounded_channel();
    let (connect_tx, connect_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            assert!(headers.len() < 4096);
            headers.push(socket.read_u8().await.unwrap());
        }
        connect_tx
            .send(String::from_utf8(headers).unwrap())
            .unwrap();
        socket
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .unwrap();
        serve(acceptor.accept(socket).await.unwrap(), requests).await;
    });
    let mut proxy = Peer {
        address,
        requests: received,
        task,
    };
    let url = Url::parse("https://service.invalid/api/").unwrap();
    let mut config = settings();
    config.insert("HTTPSProxy".into(), Value::String("127.0.0.1".into()));
    config.insert("HTTPSPort".into(), Value::Integer(address.port().into()));
    let mut tls = tls_config().unwrap();
    let verifier = rustls_platform_verifier::Verifier::new_with_extra_roots(
        [root.der().clone()],
        tls.crypto_provider().clone(),
    )
    .unwrap();
    tls.dangerous().set_certificate_verifier(Arc::new(verifier));
    let mut executor = Executor::new(url.as_str(), Some("service-token")).unwrap();
    executor.client = configure_with(
        client_builder(TimeoutOptions::default())
            .unwrap()
            .no_proxy()
            .tls_backend_preconfigured(tls),
        &url,
        &Environment::default(),
        || Ok(Some(config)),
    )
    .unwrap()
    .build()
    .unwrap();
    exercise(&executor, &mut proxy, false).await;
    let connect = timeout(DEADLINE, connect_rx).await.unwrap().unwrap();
    assert!(connect.starts_with("CONNECT service.invalid:443 HTTP/1.1\r\n"));
    assert!(!connect.to_ascii_lowercase().contains("authorization:"));
}
