// This is free and unencumbered software released into the public domain.

use super::*;
use crate::{ProxyMode, UpstreamProxyOptions};
use alloc::string::ToString as _;
use std::process::{Command, Output};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const CHILD: &str = "ASIMOV_OPENAI_PROXY_TEST";
const DEADLINE: Duration = Duration::from_secs(15);
const BODY: &[u8] = br#"{"stream":true}"#;

fn child(test: &str, case: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            &format!("openai::tests::proxy::{test}"),
            "--nocapture",
        ])
        .env(CHILD, case)
        .env_remove("REQUEST_METHOD")
        .env("no_proxy", "*")
        .env("NO_PROXY", "");
    for key in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        command.env(key, "http://127.0.0.1:1");
    }
    command
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn tls() -> Arc<rustls::ClientConfig> {
    Arc::new(
        rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth(),
    )
}

fn fixture(target: &url::Url, config: ProxyConfig) -> Result<Proxy, Error> {
    let tls = tls();
    let connector = ProxyConnector::new(target, config, Arc::clone(&tls))?;
    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config((*tls).clone())
        .https_or_http()
        .enable_http1()
        .wrap_connector(connector);
    Ok(Proxy {
        state: ProxyState {
            client: Client::builder(TokioExecutor::new()).build(connector),
            upstream_base_url: target.as_str().into(),
            authorization: authorization_header("service-key")?,
            logger: None,
            max_body_bytes: BODY.len(),
            upstream_timeout: Duration::from_secs(3),
        },
    })
}

fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then_some(value.trim())
    })
}

async fn idle(listener: &TcpListener) {
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
}

async fn socks(socket: &mut tokio::net::TcpStream, remote: bool, port: u16) {
    assert_eq!(socket.read_u8().await.unwrap(), 5);
    let mut methods = alloc::vec![0; socket.read_u8().await.unwrap() as usize];
    socket.read_exact(&mut methods).await.unwrap();
    assert!(methods.contains(&2));
    socket.write_all(&[5, 2]).await.unwrap();
    assert_eq!(socket.read_u8().await.unwrap(), 1);
    for expected in [b"user", b"pass"] {
        let mut value = alloc::vec![0; socket.read_u8().await.unwrap() as usize];
        socket.read_exact(&mut value).await.unwrap();
        assert_eq!(value, expected);
    }
    socket.write_all(&[1, 0]).await.unwrap();
    let mut head = [0; 4];
    socket.read_exact(&mut head).await.unwrap();
    assert_eq!(&head[..3], &[5, 1, 0]);
    if remote {
        assert_eq!(head[3], 3);
        let mut host = alloc::vec![0; socket.read_u8().await.unwrap() as usize];
        socket.read_exact(&mut host).await.unwrap();
        assert_eq!(host, b"service.invalid");
    } else {
        assert_eq!(head[3], 1);
        let mut ip = [0; 4];
        socket.read_exact(&mut ip).await.unwrap();
        assert_eq!(ip, [127, 0, 0, 1]);
    }
    assert_eq!(socket.read_u16().await.unwrap(), port);
    socket
        .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
        .await
        .unwrap();
}

#[tokio::test]
async fn shared_routing_preserves_credentials_streaming_and_redirects() {
    if let Ok(case) = std::env::var(CHILD) {
        let target = url::Url::parse(&std::env::var("ASIMOV_PROXY_TEST_TARGET").unwrap()).unwrap();
        let endpoint = std::env::var("ASIMOV_PROXY_TEST_ENDPOINT").unwrap();
        let config = match case.as_str() {
            "direct" => ProxyConfig::Direct,
            "explicit" => ProxyConfig::parse(&endpoint).unwrap(),
            "options-bypass" | "options-force" => ProxyConfig::from_options(
                &target,
                &UpstreamProxyOptions {
                    mode: ProxyMode::Explicit(endpoint),
                    no_proxy: Some(if case == "options-bypass" { "*" } else { "" }.into()),
                },
            )
            .unwrap(),
            _ => ProxyOptions::default().upstream_proxy,
        };
        let proxy = fixture(&target, config).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(proxy.serve(listener, async {
            let _ = stopped.await;
        }));
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(DEADLINE)
            .build()
            .unwrap();
        let mut response = client
            .post(format!("http://{address}/v1/chat/completions?test=1"))
            .header("authorization", "Bearer inbound-key")
            .header("proxy-authorization", "Basic inbound-key")
            .body(BODY)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if case == "redirect" {
                StatusCode::TEMPORARY_REDIRECT
            } else {
                StatusCode::CREATED
            }
        );
        assert_eq!(response.chunk().await.unwrap().unwrap(), "data: first\n\n");
        std::fs::write(std::env::var("ASIMOV_PROXY_TEST_READY").unwrap(), "ready").unwrap();
        assert_eq!(response.text().await.unwrap(), "data: second\n\n");
        stop.send(()).unwrap();
        tokio::time::timeout(DEADLINE, server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        return;
    }

    for (case, bypass, proxied) in [
        ("automatic", "", true),
        ("direct", "", false),
        ("cidr", "127.0.0.0/8", false),
        ("wildcard", "*", false),
        ("cgi", "", false),
        ("explicit", "*", true),
        ("options-bypass", "", false),
        ("options-force", "*", true),
        ("socks-local", "", true),
        ("socks-remote", "", true),
        ("redirect", "", true),
    ] {
        let root = tempfile::tempdir().unwrap();
        let ready = root.path().join("ready");
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = origin.local_addr().unwrap();
        let host = if case == "socks-remote" {
            "service.invalid"
        } else {
            "127.0.0.1"
        };
        let target = format!("http://{host}:{}/api", address.port());
        let scheme = match case {
            "socks-local" => "socks5",
            "socks-remote" => "socks5h",
            _ => "http",
        };
        let endpoint = format!("{scheme}://user:pass@{}", proxy.local_addr().unwrap());
        let mut command = child(
            "shared_routing_preserves_credentials_streaming_and_redirects",
            case,
        );
        command
            .env("HTTP_PROXY", &endpoint)
            .env("NO_PROXY", bypass)
            .env("ASIMOV_PROXY_TEST_TARGET", &target)
            .env("ASIMOV_PROXY_TEST_ENDPOINT", &endpoint)
            .env("ASIMOV_PROXY_TEST_READY", &ready);
        if matches!(case, "cgi" | "options-force") {
            command.env("REQUEST_METHOD", "POST");
        }
        let task = tokio::task::spawn_blocking(move || command.output().unwrap());
        let selected = if proxied { &proxy } else { &origin };
        let (mut socket, _) = tokio::time::timeout(DEADLINE, selected.accept())
            .await
            .unwrap()
            .unwrap();
        if proxied {
            if scheme.starts_with("socks") {
                socks(&mut socket, case == "socks-remote", address.port()).await;
            } else {
                let connect = read_request_headers(&mut socket).await;
                assert!(connect.starts_with(&format!("CONNECT {address} HTTP/1.1\r\n")));
                assert_eq!(
                    header(&connect, "proxy-authorization"),
                    Some("Basic dXNlcjpwYXNz")
                );
                assert!(!connect.contains("service-key"));
                socket
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await
                    .unwrap();
            }
        }
        let request = read_request_headers(&mut socket).await;
        assert!(request.starts_with("POST /api/v1/chat/completions?test=1 HTTP/1.1\r\n"));
        assert_eq!(
            header(&request, "authorization"),
            Some("Bearer service-key")
        );
        assert_eq!(header(&request, "proxy-authorization"), None);
        assert!(!request.contains("inbound-key"));
        let mut body = alloc::vec![0; BODY.len()];
        socket.read_exact(&mut body).await.unwrap();
        assert_eq!(body, BODY);
        let status = if case == "redirect" { 307 } else { 201 };
        socket.write_all(format!("HTTP/1.1 {status} Test\r\nLocation: http://redirect.invalid/\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\nd\r\ndata: first\n\n\r\n").as_bytes()).await.unwrap();
        tokio::time::timeout(DEADLINE, async {
            while !ready.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("response was buffered instead of streamed");
        socket
            .write_all(b"e\r\ndata: second\n\n\r\n0\r\n\r\n")
            .await
            .unwrap();
        assert_success(tokio::time::timeout(DEADLINE, task).await.unwrap().unwrap());
        idle(&proxy).await;
        idle(&origin).await;
    }
}

#[tokio::test]
async fn public_openrouter_uses_https_proxy_without_direct_fallback() {
    if std::env::var_os(CHILD).is_some() {
        let proxy = Proxy::openrouter("service-key", ProxyOptions::default()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(proxy.serve(listener, async {
            let _ = stopped.await;
        }));
        let response = reqwest::Client::builder()
            .no_proxy()
            .timeout(DEADLINE)
            .build()
            .unwrap()
            .post(format!("http://{address}/v1/chat/completions"))
            .body(BODY)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        stop.send(()).unwrap();
        tokio::time::timeout(DEADLINE, server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        return;
    }
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut command = child(
        "public_openrouter_uses_https_proxy_without_direct_fallback",
        "1",
    );
    command.env(
        "HTTPS_PROXY",
        format!("http://user:pass@{}", proxy.local_addr().unwrap()),
    );
    let task = tokio::task::spawn_blocking(move || command.output().unwrap());
    let (mut socket, _) = tokio::time::timeout(DEADLINE, proxy.accept())
        .await
        .unwrap()
        .unwrap();
    let request = read_request_headers(&mut socket).await;
    assert!(request.starts_with("CONNECT openrouter.ai:443 HTTP/1.1\r\n"));
    assert_eq!(
        header(&request, "proxy-authorization"),
        Some("Basic dXNlcjpwYXNz")
    );
    assert!(!request.contains("service-key"));
    socket
        .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n")
        .await
        .unwrap();
    assert_success(tokio::time::timeout(DEADLINE, task).await.unwrap().unwrap());
    idle(&proxy).await;
}

#[test]
fn legacy_discovery_uses_shared_precedence_and_bypass_rules() {
    if let Ok(case) = std::env::var(CHILD) {
        if case == "proxy" {
            assert!(matches!(
                ProxyConfig::from_env("openrouter.ai").unwrap(),
                ProxyConfig::HttpConnect { port: 3128, .. }
            ));
        } else {
            for host in ["openrouter.ai", "127.0.0.1", "::1", "[::1]"] {
                assert!(matches!(
                    ProxyConfig::from_env(host).unwrap(),
                    ProxyConfig::Direct
                ));
            }
        }
        return;
    }
    for case in ["proxy", "bypass", "cgi"] {
        let mut command = child(
            "legacy_discovery_uses_shared_precedence_and_bypass_rules",
            case,
        );
        command.env("HTTPS_PROXY", "http://proxy.invalid:3128");
        if case == "bypass" {
            command.env("NO_PROXY", "*");
        }
        if case == "cgi" {
            command.env("REQUEST_METHOD", "POST");
        }
        assert_success(command.output().unwrap());
    }
}

#[tokio::test]
async fn configuration_errors_and_origin_scope_are_enforced() {
    use tower_service::Service as _;
    assert!(matches!(
        ProxyConnector::new(
            &url::Url::parse("ftp://service.invalid/").unwrap(),
            ProxyConfig::Direct,
            tls(),
        ),
        Err(Error::Proxy(_))
    ));
    let target = url::Url::parse(UPSTREAM_BASE_URL).unwrap();
    let error = ProxyConfig::from_options(
        &target,
        &UpstreamProxyOptions {
            mode: ProxyMode::Explicit("http://user:secret@proxy.invalid/path".into()),
            no_proxy: Some("*".into()),
        },
    )
    .unwrap_err();
    assert!(matches!(error, Error::Proxy(_)));
    assert!(!error.to_string().contains("secret"));
    assert!(matches!(
        ProxyConfig::from_options(
            &target,
            &UpstreamProxyOptions {
                mode: ProxyMode::Explicit("socks4://proxy.invalid".into()),
                no_proxy: Some(String::new()),
            }
        ),
        Err(Error::UnsupportedProxyScheme(_))
    ));
    let mut connector = ProxyConnector::new(&target, ProxyConfig::Direct, tls()).unwrap();
    let result = connector
        .call("https://other.invalid/".parse().unwrap())
        .await;
    let Err(error) = result else {
        panic!("connector escaped its bound origin")
    };
    assert!(error.downcast_ref::<asimov_http::proxy::Error>().is_some());
}
