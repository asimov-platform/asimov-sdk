// This is free and unencumbered software released into the public domain.

use super::*;

#[test]
fn explicit_endpoints_validate_before_bypass_discovery_tls_or_identity() {
    for (input, scheme, host) in [
        ("proxy.invalid:3128", "http", "proxy.invalid"),
        ("http://proxy.invalid", "http", "proxy.invalid"),
        ("https://proxy.invalid:8443/", "https", "proxy.invalid"),
        ("socks4://127.0.0.1", "socks4", "127.0.0.1"),
        ("socks4a://proxy.invalid:1080", "socks4a", "proxy.invalid"),
        ("socks5://[::1]:1080", "socks5", "[::1]"),
        (
            "socks5h://User%40Realm:p%3Aa%25ss+@proxy.invalid",
            "socks5h",
            "proxy.invalid",
        ),
    ] {
        let options = ProxyOptions {
            mode: ProxyMode::Explicit(input.into()),
            no_proxy: Some(String::new()),
        };
        let configuration = resolve_options(
            &options,
            || panic!("must not capture environment"),
            |_| panic!("must not read native settings"),
        )
        .unwrap();
        let proxy = configuration.proxy.unwrap();
        assert_eq!(proxy.scheme(), scheme);
        assert_eq!(proxy.host_str(), Some(host));
    }
    for input in [
        "",
        " ",
        "http://",
        "http://[::1",
        "http://proxy.invalid:65536",
        "http://proxy.invalid:0",
        "socks5://proxy.invalid:0",
        "ftp://User:Secret@proxy.invalid",
        "socks://proxy.invalid",
        "file:///proxy.invalid",
        "http://User:Secret@proxy.invalid/path",
        "http://proxy.invalid?Secret",
        "https://proxy.invalid#Secret",
        "http://User:Secret@proxy.invalid:bad",
        " http://proxy.invalid",
        "http://proxy.invalid\n",
        "http://user:sec\tret@proxy.invalid",
        "socks5h://bücher.invalid",
        "socks5h://b%C3%BCcher.invalid",
    ] {
        let options = ProxyOptions {
            mode: ProxyMode::Explicit(input.into()),
            // Validation must not be conditional on the chosen route.
            no_proxy: Some("*".into()),
        };
        let error = resolve_options(
            &options,
            || panic!("must not capture environment"),
            |_| panic!("must not read native settings"),
        )
        .map(|_| ())
        .expect_err(input);
        assert!(matches!(error, Error::InvalidProxyUrl(_)), "{input}");
        assert!(!format!("{error:?}: {error}").contains("Secret"));
        assert!(core::error::Error::source(&error).is_none());
        // This public path must reject the URL before initializing local keys.
        assert!(matches!(
            Executor::new_with_options(
                "https://service.invalid",
                None,
                TransportOptions {
                    proxy: options,
                    ..TransportOptions::default()
                }
            ),
            Err(Error::InvalidProxyUrl(_))
        ));
    }
}

#[test]
fn option_debug_output_never_includes_explicit_url_credentials() {
    for endpoint in [
        "http://PrivateUser:PrivatePassword@proxy.invalid:3128",
        "invalid://PrivateUser:PrivatePassword@proxy.invalid:bad",
    ] {
        let options = TransportOptions {
            proxy: ProxyOptions {
                mode: ProxyMode::Explicit(endpoint.into()),
                no_proxy: None,
            },
            ..TransportOptions::default()
        };
        let debug = format!("{options:?}");
        assert!(debug.contains("Explicit([REDACTED])"));
        assert!(!debug.contains("PrivateUser"));
        assert!(!debug.contains("PrivatePassword"));
    }
}

#[test]
fn modes_read_only_the_settings_they_inherit() {
    for no_proxy in [None, Some("*".into()), Some(String::new())] {
        let configuration = resolve_options(
            &ProxyOptions {
                mode: ProxyMode::Direct,
                no_proxy,
            },
            || panic!("direct must not read environment"),
            |_| panic!("direct must not read native settings"),
        )
        .unwrap();
        assert!(configuration.proxy.is_none());
    }
    let configuration = resolve_options(
        &ProxyOptions {
            mode: ProxyMode::Explicit("http://explicit.invalid:3128".into()),
            no_proxy: None,
        },
        || Environment {
            proxy: "http://environment.invalid:8080".into(),
            all_proxy: "http://fallback.invalid:8080".into(),
            no_proxy: "intranet.invalid".into(),
            cgi: true,
            ..Environment::default()
        },
        |_| panic!("explicit must not read native settings"),
    )
    .unwrap();
    assert_eq!(
        configuration.proxy.unwrap().host_str(),
        Some("explicit.invalid")
    );
    assert_eq!(configuration.no_proxy, "intranet.invalid");
    // Automatic mode retains its tolerant legacy environment fallback.
    let configuration = resolve_options(
        &ProxyOptions::default(),
        || Environment {
            proxy: "unsupported://invalid".into(),
            all_proxy: "http://fallback.invalid:8080".into(),
            ..Environment::default()
        },
        |_| panic!("per-scheme environment override suppresses native settings"),
    )
    .unwrap();
    assert_eq!(
        configuration.proxy.unwrap().host_str(),
        Some("fallback.invalid")
    );
}

#[test]
fn automatic_bypass_replacement_distinguishes_inherited_empty_and_wildcard() {
    let url = Url::parse("https://intranet").unwrap();
    let native_mode = proxy_watch::parse::windows_manual("proxy.invalid:3128", "<local>");
    for (inherited, replacement, proxied) in [
        ("", None, false),
        ("other.invalid", None, true),
        ("", Some(""), true),
        ("*", Some(""), true),
        ("*", Some("other.invalid"), true),
        ("other.invalid", Some("*"), false),
    ] {
        let options = ProxyOptions {
            mode: ProxyMode::Automatic,
            no_proxy: replacement.map(String::from),
        };
        let configuration = resolve_options(
            &options,
            || Environment {
                no_proxy: inherited.into(),
                ..Environment::default()
            },
            |env| {
                assert_ne!(
                    replacement,
                    Some("*"),
                    "wildcard must skip native discovery"
                );
                native::route(&native_mode, &url, env, "ProxyServer")
            },
        )
        .unwrap();
        assert_eq!(
            configuration.proxy.is_some(),
            proxied,
            "{inherited:?}, {replacement:?}"
        );
        assert_eq!(configuration.no_proxy, replacement.unwrap_or(inherited));
    }
    let options = ProxyOptions {
        no_proxy: Some(String::new()),
        ..ProxyOptions::default()
    };
    assert!(matches!(
        resolve_options(&options, Environment::default, |_| Err(
            Error::InvalidProxy("native")
        )),
        Err(Error::InvalidProxy("native"))
    ));
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
            .args(["--exact", "http::proxy::tests::configuration::public_configuration_routes_clones_and_discovery", "--nocapture"])
            .env(CHILD, "1")
            .env("HTTP_PROXY", "http://127.0.0.1:1")
            .env("HTTPS_PROXY", "http://127.0.0.1:1")
            .env("ALL_PROXY", "http://127.0.0.1:1")
            .env("NO_PROXY", "*")
            .env("REQUEST_METHOD", "")
            .output().unwrap();
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
                    serve(Box::new(socket), proxied, proxied).await;
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
                // An independent direct executor for the very same origin can coexist
                // with the proxied pool, using the same unchanged process environment.
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
                        serve(Box::new(socket), false, false).await;
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
