// This is free and unencumbered software released into the public domain.

use super::*;
use crate::http::{Executor, StreamExt, TimeoutOptions, TransportOptions, client_builder};
use alloc::{sync::Arc, vec, vec::Vec};
use core::{convert::Infallible, time::Duration};
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, body::Bytes, service::service_fn};
use hyper_util::rt::TokioIo;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};

const DEADLINE: Duration = Duration::from_secs(10);

mod configuration;

#[test]
fn overrides_and_cgi_skip_native_discovery() {
    for (env, expected) in [
        (
            Environment {
                proxy: "socks5h://proxy.invalid".into(),
                ..Environment::default()
            },
            Route::Automatic,
        ),
        (
            Environment {
                proxy: "unsupported://proxy.invalid".into(),
                ..Environment::default()
            },
            Route::Automatic,
        ),
        (
            Environment {
                no_proxy: "other.invalid, *".into(),
                ..Environment::default()
            },
            Route::Direct,
        ),
        (
            Environment {
                cgi: true,
                ..Environment::default()
            },
            Route::Direct,
        ),
    ] {
        assert_eq!(
            select(&env, || panic!("native settings must not be read")).unwrap(),
            expected
        );
    }
}

#[test]
fn environment_urls_keep_transport_dns_modes_and_credentials() {
    for scheme in ["http", "https", "socks4", "socks4a", "socks5", "socks5h"] {
        let url = environment_proxy(&format!(
            "{scheme}://User%40Realm:p%3Aa%25ss+@proxy.invalid:1080"
        ))
        .unwrap();
        assert_eq!(url.scheme(), scheme);
        assert_eq!(url.username(), "User%40Realm");
        assert_eq!(url.password(), Some("p%3Aa%25ss+"));
    }
    assert_eq!(
        environment_proxy("proxy.invalid:3128").unwrap().scheme(),
        "http"
    );
    for value in [
        "",
        "ftp://proxy.invalid",
        "http://proxy.invalid:bad",
        "http://",
    ] {
        assert!(environment_proxy(value).is_none());
    }
}

#[test]
fn environment_capture_uses_uppercase_first_including_empty_values() {
    const CHILD: &str = "ASIMOV_PROXY_ENV_TEST";
    if let Ok(case) = std::env::var(CHILD) {
        let http = Environment::capture(&Url::parse("http://service.invalid").unwrap());
        let https = Environment::capture(&Url::parse("https://service.invalid").unwrap());
        if case == "empty" {
            assert!(http.proxy.is_empty() && https.proxy.is_empty());
            assert!(http.all_proxy.is_empty() && http.no_proxy.is_empty());
            assert!(http.cgi);
        } else {
            assert_eq!(http.proxy, format!("{case}-http"));
            assert_eq!(https.proxy, format!("{case}-https"));
            assert_eq!(http.all_proxy, format!("{case}-all"));
            assert_eq!(http.no_proxy, format!("{case}-no"));
            assert!(!http.cgi);
        }
        return;
    }
    for case in ["upper", "lower", "empty"] {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.args(["--exact", "http::proxy::tests::environment_capture_uses_uppercase_first_including_empty_values", "--nocapture"])
            .env(CHILD, case).env_remove("REQUEST_METHOD");
        for (upper, lower, suffix) in [
            ("HTTP_PROXY", "http_proxy", "http"),
            ("HTTPS_PROXY", "https_proxy", "https"),
            ("ALL_PROXY", "all_proxy", "all"),
            ("NO_PROXY", "no_proxy", "no"),
        ] {
            child.env_remove(upper).env_remove(lower);
            if case == "lower" || !cfg!(windows) {
                child.env(lower, format!("lower-{suffix}"));
            }
            if case != "lower" {
                child.env(
                    upper,
                    if case == "empty" {
                        String::new()
                    } else {
                        format!("upper-{suffix}")
                    },
                );
            }
        }
        if case == "empty" {
            child.env("REQUEST_METHOD", "");
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "{case}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

trait Io: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Io for T {}
type Stream = alloc::boxed::Box<dyn Io>;

fn tls() -> (
    rustls::ClientConfig,
    tokio_rustls::TlsAcceptor,
    tokio_rustls::TlsAcceptor,
) {
    let mut root_params = rcgen::CertificateParams::default();
    root_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    root_params.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign];
    let root_key = rcgen::KeyPair::generate().unwrap();
    let root = root_params.self_signed(&root_key).unwrap();
    let issuer = rcgen::Issuer::new(root_params, root_key);
    let server = |names: &[&str]| {
        let key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(
            names.iter().map(|s| (*s).into()).collect::<Vec<String>>(),
        )
        .unwrap();
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        let cert = params.signed_by(&key, &issuer).unwrap();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        )
        .unwrap();
        tokio_rustls::TlsAcceptor::from(Arc::new(config))
    };
    let mut client = crate::http::tls_config().unwrap();
    let verifier = rustls_platform_verifier::Verifier::new_with_extra_roots(
        [root.der().clone()],
        client.crypto_provider().clone(),
    )
    .unwrap();
    client
        .dangerous()
        .set_certificate_verifier(Arc::new(verifier));
    (
        client,
        server(&["proxy.invalid"]),
        server(&["service.invalid", "localhost"]),
    )
}

async fn headers(io: &mut Stream) -> String {
    let mut data = Vec::new();
    while !data.ends_with(b"\r\n\r\n") {
        assert!(data.len() < 8192);
        data.push(io.read_u8().await.unwrap());
    }
    String::from_utf8(data).unwrap()
}

async fn socks(io: &mut Stream, scheme: &str, destination: &str, port: u16) {
    if scheme.starts_with("socks5") {
        assert_eq!(io.read_u8().await.unwrap(), 5);
        let count = io.read_u8().await.unwrap();
        let mut methods = vec![0; count as usize];
        io.read_exact(&mut methods).await.unwrap();
        assert!(methods.contains(&2));
        io.write_all(&[5, 2]).await.unwrap();
        assert_eq!(io.read_u8().await.unwrap(), 1);
        let count = io.read_u8().await.unwrap();
        let mut user = vec![0; count as usize];
        io.read_exact(&mut user).await.unwrap();
        let count = io.read_u8().await.unwrap();
        let mut password = vec![0; count as usize];
        io.read_exact(&mut password).await.unwrap();
        assert_eq!(user, b"User@Realm");
        assert_eq!(password, b"p:a%ss+");
        io.write_all(&[1, 0]).await.unwrap();
        let mut request = [0; 4];
        io.read_exact(&mut request).await.unwrap();
        assert_eq!(&request[..3], &[5, 1, 0]);
        if scheme == "socks5h" {
            assert_eq!(request[3], 3);
            let count = io.read_u8().await.unwrap();
            let mut host = vec![0; count as usize];
            io.read_exact(&mut host).await.unwrap();
            assert_eq!(host, destination.as_bytes());
        } else {
            assert!(matches!(request[3], 1 | 4));
            let mut ip = vec![0; if request[3] == 1 { 4 } else { 16 }];
            io.read_exact(&mut ip).await.unwrap();
        }
        assert_eq!(io.read_u16().await.unwrap(), port);
        io.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
            .await
            .unwrap();
    } else {
        assert_eq!(io.read_u8().await.unwrap(), 4);
        assert_eq!(io.read_u8().await.unwrap(), 1);
        assert_eq!(io.read_u16().await.unwrap(), port);
        let mut ip = [0; 4];
        io.read_exact(&mut ip).await.unwrap();
        while io.read_u8().await.unwrap() != 0 {}
        if scheme == "socks4a" {
            assert_eq!(&ip[..3], &[0, 0, 0]);
            assert_ne!(ip[3], 0);
            let mut host = Vec::new();
            loop {
                let byte = io.read_u8().await.unwrap();
                if byte == 0 {
                    break;
                }
                host.push(byte);
            }
            assert_eq!(host, destination.as_bytes());
        } else {
            assert_eq!(ip, [127, 0, 0, 1]);
        }
        io.write_all(&[0, 90, 0, 0, 127, 0, 0, 1]).await.unwrap();
    }
}

async fn serve(io: Stream, forwarded: bool, proxy_auth: bool) {
    use base64::Engine as _;
    let auth = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("User@Realm:p:a%ss+")
    );
    let service = service_fn(move |request: Request<hyper::body::Incoming>| {
        let auth = auth.clone();
        async move {
            assert_eq!(request.uri().scheme().is_some(), forwarded);
            if proxy_auth {
                assert_eq!(request.headers()["proxy-authorization"], auth);
            } else {
                assert!(!request.headers().contains_key("proxy-authorization"));
            }
            let discovery = request.uri().path() == "/.well-known/jwks.json";
            if discovery {
                assert!(!request.headers().contains_key("authorization"));
            } else {
                assert_eq!(request.headers()["authorization"], "Bearer service-token");
                assert!(request.headers().contains_key("content-digest"));
            }
            request.into_body().collect().await.unwrap();
            let body = if discovery {
                br#"{"keys":[{"kty":"OKP","crv":"Ed25519","x":"11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"}]}"#.as_slice()
            } else {
                b"{}\n"
            };
            Ok::<_, Infallible>(Response::new(Full::new(Bytes::copy_from_slice(body))))
        }
    });
    let result = hyper::server::conn::http1::Builder::new()
        .serve_connection(TokioIo::new(io), service)
        .await;
    if let Err(error) = result {
        // The client may close before the server writes TLS close_notify.
        assert!(error.is_shutdown(), "{error}");
    }
}

#[tokio::test]
async fn authenticated_http_https_and_socks_proxies_carry_discovery_and_execution() {
    use base64::Engine as _;
    for scheme in ["http", "https", "socks4", "socks4a", "socks5", "socks5h"] {
        for secure in [false, true] {
            let (tls, proxy_tls, origin_tls) = tls();
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let destination = if matches!(scheme, "socks4" | "socks5") {
                "localhost"
            } else {
                "service.invalid"
            };
            let port = if secure { 443 } else { 80 };
            let forwarded = !secure && matches!(scheme, "http" | "https");
            let task = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                let mut io: Stream = if scheme == "https" {
                    alloc::boxed::Box::new(proxy_tls.accept(socket).await.unwrap())
                } else {
                    alloc::boxed::Box::new(socket)
                };
                if scheme.starts_with("socks") {
                    socks(&mut io, scheme, destination, port).await;
                } else if secure {
                    let connect = headers(&mut io).await;
                    assert!(
                        connect.starts_with(&format!("CONNECT {destination}:443 HTTP/1.1\r\n"))
                    );
                    let auth =
                        base64::engine::general_purpose::STANDARD.encode("User@Realm:p:a%ss+");
                    let authorization = connect.lines().find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("proxy-authorization")
                            .then_some(value.trim())
                    });
                    assert_eq!(authorization, Some(format!("Basic {auth}").as_str()));
                    assert!(!connect.contains("service-token"));
                    io.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                        .await
                        .unwrap();
                }
                if secure {
                    io = alloc::boxed::Box::new(origin_tls.accept(io).await.unwrap());
                }
                serve(io, forwarded, forwarded).await;
            });
            let url = format!(
                "{}://{destination}/api/",
                if secure { "https" } else { "http" }
            );
            let proxy = Url::parse(&format!(
                "{scheme}://User%40Realm:p%3Aa%25ss+@proxy.invalid:{}",
                address.port()
            ))
            .unwrap();
            let mut executor = Executor::new(&url, Some("service-token")).unwrap();
            executor.client = resolve_options(
                &ProxyOptions {
                    mode: ProxyMode::Explicit(proxy.as_str().into()),
                    no_proxy: Some(String::new()),
                },
                || panic!("explicit proxy and bypass must not read the environment"),
                |_| panic!("explicit proxy must not read native settings"),
            )
            .unwrap()
            .apply(
                client_builder(TimeoutOptions::default())
                    .unwrap()
                    .tls_backend_preconfigured(tls)
                    .resolve("proxy.invalid", address),
            )
            .unwrap()
            .build()
            .unwrap();
            timeout(DEADLINE, async {
                executor.remote_public_key().await.unwrap();
                let mut response = executor
                    .post_jsonl("fetch", &serde_json::json!({}))
                    .await
                    .unwrap();
                assert!(response.next().await.unwrap().is_ok());
                assert!(response.next().await.is_none());
            })
            .await
            .unwrap_or_else(|_| panic!("{scheme}, secure={secure} timed out"));
            drop(executor);
            timeout(DEADLINE, task).await.unwrap().unwrap();
        }
    }
}

#[tokio::test]
async fn proxy_and_tunneled_origin_tls_validate_their_own_hostnames() {
    for outer_tls in [false, true] {
        let (tls, proxy_tls, origin_tls) = tls();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut io: Stream = Box::new(socket);
            if outer_tls {
                // A certificate valid for the service cannot identify the proxy.
                assert!(origin_tls.accept(io).await.is_err());
            } else {
                let connect = headers(&mut io).await;
                assert!(connect.starts_with("CONNECT service.invalid:443 HTTP/1.1\r\n"));
                io.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await
                    .unwrap();
                // And a proxy's certificate cannot identify the tunneled service.
                assert!(proxy_tls.accept(io).await.is_err());
            }
        });
        let proxy = Url::parse(&format!(
            "{}://proxy.invalid:{}",
            if outer_tls { "https" } else { "http" },
            address.port()
        ))
        .unwrap();
        let client = configure_with(
            client_builder(TimeoutOptions::default())
                .unwrap()
                .tls_backend_preconfigured(tls)
                .resolve("proxy.invalid", address),
            &Environment::default(),
            || Ok(Route::Native(proxy)),
        )
        .unwrap()
        .build()
        .unwrap();
        let error = timeout(DEADLINE, client.get("https://service.invalid/").send())
            .await
            .unwrap()
            .unwrap_err();
        assert!(error.is_connect() && !error.is_timeout(), "{error}");
        timeout(DEADLINE, task).await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn environment_fallback_and_bypass_control_the_actual_connection() {
    for (per_scheme, bypass, native, cgi, proxied) in [
        ("selected", "", Route::Automatic, false, true),
        ("", "", Route::Automatic, false, true),
        (
            "unsupported://bad.invalid",
            "",
            Route::Automatic,
            false,
            true,
        ),
        ("", "", Route::Direct, false, false),
        ("", "*", Route::Automatic, false, false),
        ("", "127.0.0.0/8", Route::Automatic, false, false),
        ("", "", Route::Automatic, true, false),
    ] {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", origin.local_addr().unwrap());
        let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
        let env = Environment {
            proxy: if per_scheme == "selected" {
                proxy_url.clone()
            } else {
                per_scheme.into()
            },
            all_proxy: if per_scheme == "selected" {
                "http://127.0.0.1:1".into()
            } else {
                proxy_url
            },
            no_proxy: bypass.into(),
            cgi,
            ..Environment::default()
        };
        let client = configure_with(reqwest::Client::builder(), &env, || {
            assert_ne!(per_scheme, "selected");
            Ok(native)
        })
        .unwrap()
        .build()
        .unwrap();
        let request = tokio::spawn(async move { client.get(url).send().await.unwrap() });
        let chosen = if proxied { &proxy } else { &origin };
        let (socket, _) = timeout(DEADLINE, chosen.accept()).await.unwrap().unwrap();
        let mut io: Stream = alloc::boxed::Box::new(socket);
        let header = headers(&mut io).await;
        assert_eq!(header.starts_with("GET http://"), proxied);
        io.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        assert_eq!(
            timeout(DEADLINE, request).await.unwrap().unwrap().status(),
            200
        );
    }
}
