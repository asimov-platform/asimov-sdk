// This is free and unencumbered software released into the public domain.

use super::*;
use alloc::format;

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
    let configuration = resolve_options(
        &ProxyOptions::default(),
        || Environment {
            proxy: "unsupported://invalid".into(),
            all_proxy: "http://fallback.invalid:8080".into(),
            ..Environment::default()
        },
        |_| panic!("per-scheme override suppresses native settings"),
    )
    .unwrap();
    assert_eq!(
        configuration.proxy.unwrap().host_str(),
        Some("fallback.invalid")
    );
    assert!(matches!(
        resolve_options(
            &ProxyOptions {
                mode: ProxyMode::Explicit("ftp://User:Secret@proxy.invalid".into()),
                no_proxy: Some("*".into())
            },
            || panic!("invalid explicit URL must not read environment"),
            |_| panic!("invalid explicit URL must not read native settings"),
        ),
        Err(Error::InvalidProxyUrl(_))
    ));
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
        let configuration = resolve_options(
            &ProxyOptions {
                mode: ProxyMode::Automatic,
                no_proxy: replacement.map(String::from),
            },
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
    assert!(matches!(
        resolve_options(
            &ProxyOptions {
                no_proxy: Some(String::new()),
                ..ProxyOptions::default()
            },
            Environment::default,
            |_| Err(Error::InvalidProxy("native")),
        ),
        Err(Error::InvalidProxy("native"))
    ));
}

#[test]
fn origin_binding_excludes_paths_and_credentials_but_checks_scheme_host_and_port() {
    let policy = OriginProxy::resolve(
        &Url::parse("https://User:Secret@service.invalid:443/api/").unwrap(),
        &ProxyOptions {
            mode: ProxyMode::Direct,
            no_proxy: None,
        },
    )
    .unwrap();
    assert_eq!(
        policy.origin().ascii_serialization(),
        "https://service.invalid"
    );
    assert!(!format!("{policy:?}").contains("Secret"));
    policy
        .check_origin(&Url::parse("https://service.invalid/another?query").unwrap())
        .unwrap();
    for url in [
        "http://service.invalid",
        "https://service.invalid:8443",
        "https://other.invalid",
        "file:///tmp/test",
    ] {
        assert!(matches!(
            policy.check_origin(&Url::parse(url).unwrap()),
            Err(Error::InvalidOrigin(_))
        ));
    }
    assert!(matches!(
        OriginProxy::resolve(
            &Url::parse("file:///tmp/test").unwrap(),
            &ProxyOptions::default()
        ),
        Err(Error::InvalidOrigin(_))
    ));
}

#[test]
fn backend_neutral_settings_keep_bypass_rules_and_redact_proxy_userinfo() {
    let policy = OriginProxy::resolve(
        &Url::parse("https://service.invalid/api").unwrap(),
        &ProxyOptions {
            mode: ProxyMode::Explicit(
                "http://PrivateUser:PrivatePassword@proxy.invalid:3128".into(),
            ),
            no_proxy: Some("service.invalid".into()),
        },
    )
    .unwrap();
    assert_eq!(
        policy.proxy_url().unwrap().host_str(),
        Some("proxy.invalid")
    );
    assert_eq!(policy.no_proxy(), "service.invalid");
    let debug = format!("{policy:?}");
    assert!(!debug.contains("PrivateUser") && !debug.contains("PrivatePassword"));
}

#[test]
fn environment_capture_uses_uppercase_first_including_empty_values() {
    const CHILD: &str = "ASIMOV_HTTP_PROXY_ENV_TEST";
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
        child.args(["--exact", "proxy::system::tests::environment_capture_uses_uppercase_first_including_empty_values", "--nocapture"])
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

#[cfg(feature = "reqwest")]
#[tokio::test]
async fn environment_fallback_and_bypass_control_the_actual_connection() {
    use alloc::{sync::Arc, vec::Vec};
    use core::time::Duration;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        time::timeout,
    };
    let deadline = Duration::from_secs(5);
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
        let origin = Arc::new(TcpListener::bind("127.0.0.1:0").await.unwrap());
        let proxy = Arc::new(TcpListener::bind("127.0.0.1:0").await.unwrap());
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
        let configuration = resolve_environment(&env, || {
            assert_ne!(per_scheme, "selected");
            Ok(native)
        })
        .unwrap();
        let client = crate::proxy::reqwest::apply(reqwest::Client::builder(), &configuration)
            .unwrap()
            .build()
            .unwrap();
        let request = tokio::spawn(async move { client.get(url).send().await.unwrap() });
        let (mut io, _) = timeout(
            deadline,
            if proxied {
                proxy.accept()
            } else {
                origin.accept()
            },
        )
        .await
        .unwrap()
        .unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(io.read_u8().await.unwrap());
        }
        assert_eq!(header.starts_with(b"GET http://"), proxied);
        io.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        assert_eq!(
            timeout(deadline, request).await.unwrap().unwrap().status(),
            200
        );
    }
}
