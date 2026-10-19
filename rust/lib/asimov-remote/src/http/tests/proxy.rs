// This is free and unencumbered software released into the public domain.

use crate::http::{
    Error, Executor, ProxyMode, ProxyOptions, StreamExt, TimeoutOptions, TransportOptions,
};
use alloc::{boxed::Box, format, string::String, sync::Arc};
use core::{convert::Infallible, time::Duration};
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, body::Bytes, service::service_fn};
use hyper_util::rt::TokioIo;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
    time::timeout,
};

const DEADLINE: Duration = Duration::from_secs(10);

#[test]
fn shared_proxy_types_and_errors_preserve_the_remote_api() {
    let proxy: asimov_http::proxy::ProxyOptions = ProxyOptions {
        mode: ProxyMode::Explicit("ftp://PrivateUser:PrivatePassword@proxy.invalid".into()),
        no_proxy: Some("*".into()),
    };
    let options = TransportOptions {
        proxy,
        ..TransportOptions::default()
    };
    let debug = format!("{options:?}");
    assert!(debug.contains("Explicit([REDACTED])"));
    assert!(!debug.contains("PrivateUser") && !debug.contains("PrivatePassword"));
    // Invalid proxies fail even when bypassed, before TLS setup or key access.
    let error = Executor::new_with_options("https://service.invalid", None, options).unwrap_err();
    assert!(matches!(error, Error::InvalidProxyUrl(_)));
    assert!(!format!("{error:?}: {error}").contains("PrivatePassword"));

    use asimov_http::proxy::Error as Shared;
    assert!(matches!(
        Error::from(Shared::InvalidProxy("HTTPProxy")),
        Error::InvalidProxy("HTTPProxy")
    ));
    assert!(matches!(
        Error::from(Shared::UnsupportedProxy("portal")),
        Error::UnsupportedProxy("portal")
    ));
    assert!(matches!(
        Error::from(Shared::InvalidOrigin("origin")),
        Error::InvalidBaseUrl(_)
    ));
}

trait Io: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Io for T {}

async fn serve(io: Box<dyn Io>, forwarded: bool) {
    use base64::Engine as _;
    let auth = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("User@Realm:p:a%ss+")
    );
    let service = service_fn(move |request: Request<hyper::body::Incoming>| {
        let auth = auth.clone();
        async move {
            assert_eq!(request.uri().scheme().is_some(), forwarded);
            if forwarded {
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
        assert!(error.is_shutdown(), "{error}");
    }
}

async fn exercise(executor: &Executor) {
    timeout(DEADLINE, async {
        let clone = executor.clone();
        clone.remote_public_key().await.unwrap();
        let mut response = executor
            .post_jsonl("fetch", &serde_json::json!({}))
            .await
            .unwrap();
        assert!(response.next().await.unwrap().is_ok());
        assert!(response.next().await.is_none());
    })
    .await
    .unwrap();
}

#[test]
fn public_configuration_routes_clones_and_discovery() {
    const CHILD: &str = "ASIMOV_EXECUTOR_PROXY_TEST";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "http::tests::proxy::public_configuration_routes_clones_and_discovery",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("HTTP_PROXY", "http://127.0.0.1:1")
            .env("HTTPS_PROXY", "http://127.0.0.1:1")
            .env("ALL_PROXY", "http://127.0.0.1:1")
            .env("NO_PROXY", "*")
            .env("REQUEST_METHOD", "")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            for (mode, bypass, proxied) in [
                ("automatic", None, false),
                ("automatic", Some(""), false), // CGI protection still applies.
                ("direct", Some(""), false),
                ("explicit", None, false),    // Inherits NO_PROXY=*.
                ("explicit", Some(""), true), // Overrides bypass and CGI defaults.
                ("explicit", Some("other.invalid"), true),
                ("explicit", Some("127.0.0.0/8"), false),
                ("explicit", Some("*"), false),
                ("legacy", None, false),
                ("legacy-timeouts", None, false),
            ] {
                let origin = Arc::new(TcpListener::bind("127.0.0.1:0").await.unwrap());
                let proxy = Arc::new(TcpListener::bind("127.0.0.1:0").await.unwrap());
                let url = format!("http://{}/api/", origin.local_addr().unwrap());
                let endpoint = format!(
                    "http://User%40Realm:p%3Aa%25ss+@{}",
                    proxy.local_addr().unwrap()
                );
                let selected = Arc::clone(if proxied { &proxy } else { &origin });
                let task = tokio::spawn(async move {
                    let (socket, _) = selected.accept().await.unwrap();
                    serve(Box::new(socket), proxied).await;
                });
                let timeouts = TimeoutOptions {
                    connect: Some(Duration::from_secs(2)),
                    ..TimeoutOptions::default()
                };
                let executor = match mode {
                    "legacy" => Executor::new(&url, Some("service-token")),
                    "legacy-timeouts" => {
                        Executor::new_with_timeouts(&url, Some("service-token"), timeouts)
                    },
                    _ => Executor::new_with_options(
                        &url,
                        Some("service-token"),
                        TransportOptions {
                            timeouts,
                            proxy: ProxyOptions {
                                mode: match mode {
                                    "automatic" => ProxyMode::Automatic,
                                    "direct" => ProxyMode::Direct,
                                    _ => ProxyMode::Explicit(endpoint),
                                },
                                no_proxy: bypass.map(String::from),
                            },
                        },
                    ),
                }
                .unwrap();
                assert_eq!(
                    executor.timeouts(),
                    if mode == "legacy" {
                        TimeoutOptions::default()
                    } else {
                        timeouts
                    }
                );
                // Independent direct and proxied pools for the same origin coexist.
                if proxied {
                    let direct = Executor::new_with_options(
                        &url,
                        Some("service-token"),
                        TransportOptions {
                            proxy: ProxyOptions {
                                mode: ProxyMode::Direct,
                                ..ProxyOptions::default()
                            },
                            ..TransportOptions::default()
                        },
                    )
                    .unwrap();
                    let task = tokio::spawn(async move {
                        let (socket, _) = origin.accept().await.unwrap();
                        serve(Box::new(socket), false).await;
                    });
                    exercise(&direct).await;
                    drop(direct);
                    timeout(DEADLINE, task).await.unwrap().unwrap();
                }
                exercise(&executor).await;
                drop(executor);
                timeout(DEADLINE, task).await.unwrap().unwrap();
            }
        });
}
