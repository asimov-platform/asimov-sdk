// This is free and unencumbered software released into the public domain.

use super::*;
use alloc::{format, string::String, sync::Arc, vec};
use futures_lite::StreamExt;
use iroh::{RelayMode, endpoint::presets::Minimal};
use std::process::{Command, Output};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

const CHILD: &str = "ASIMOV_PKARR_TEST";
const URL: &str = "ASIMOV_PKARR_TEST_URL";
const GATE: &str = "ASIMOV_PKARR_TEST_GATE";
const DEADLINE: Duration = Duration::from_secs(10);

fn child(test: &str, case: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        &format!("presets::pkarr::tests::{test}"),
        "--nocapture",
    ]);
    for key in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
        "REQUEST_METHOD",
        "IROH_FORCE_STAGING_RELAYS",
    ] {
        command.env_remove(key);
    }
    command
        .env(CHILD, case)
        .env("NO_PROXY", "*")
        .env("HTTP_PROXY", "invalid://skip-native")
        .env("HTTPS_PROXY", "invalid://skip-native");
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

async fn endpoint() -> Endpoint {
    Endpoint::builder(Minimal)
        .secret_key(key())
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")
        .unwrap()
        .bind()
        .await
        .unwrap()
}

fn key() -> SecretKey {
    SecretKey::from_bytes(&[42; 32])
}

fn info() -> EndpointInfo {
    EndpointInfo::new(key().public()).with_relay_url("https://relay.invalid/".parse().unwrap())
}

fn packet() -> SignedPacket {
    info()
        .to_pkarr_signed_packet(&key(), DEFAULT_PKARR_TTL)
        .unwrap()
}

async fn listener() -> TcpListener {
    TcpListener::bind("127.0.0.1:0").await.unwrap()
}

fn url(listener: &TcpListener) -> Url {
    format!("http://{}/pkarr", listener.local_addr().unwrap())
        .parse()
        .unwrap()
}

fn header<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

async fn request(stream: &mut (impl AsyncRead + Unpin)) -> (String, Vec<u8>) {
    timeout(DEADLINE, async {
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            assert!(headers.len() < 16384);
            headers.push(stream.read_u8().await.unwrap());
        }
        let headers = String::from_utf8(headers).unwrap();
        let length = header(&headers, "content-length")
            .map(|s| s.parse().unwrap())
            .unwrap_or(0);
        assert!(length <= SignedPacket::MAX_BYTES);
        let mut body = vec![0; length];
        stream.read_exact(&mut body).await.unwrap();
        (headers, body)
    })
    .await
    .unwrap()
}

async fn respond(stream: &mut (impl AsyncWrite + Unpin), status: &str, headers: &str, body: &[u8]) {
    stream
        .write_all(
            format!(
                "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    stream.write_all(body).await.unwrap();
    stream.flush().await.unwrap();
}

async fn receive(
    listener: &TcpListener,
    status: &str,
    headers: &str,
    body: &[u8],
) -> (String, Vec<u8>) {
    let (mut stream, _) = timeout(DEADLINE, listener.accept()).await.unwrap().unwrap();
    let received = request(&mut stream).await;
    respond(&mut stream, status, headers, body).await;
    received
}

async fn assert_idle(listener: &TcpListener) {
    assert!(
        timeout(Duration::from_millis(150), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn proxy_redirects_preserve_records_and_reselect_each_origin() {
    const TEST: &str = "proxy_redirects_preserve_records_and_reselect_each_origin";
    if std::env::var_os(CHILD).is_some() {
        let endpoint = endpoint().await;
        let client =
            Client::new("http://source.invalid/pkarr".parse().unwrap(), &endpoint).unwrap();
        client.publish(&packet()).await.unwrap();
        assert_eq!(
            client
                .resolve(key().public())
                .await
                .unwrap()
                .endpoint_info(),
            &info()
        );
        endpoint.close().await;
        return;
    }
    let proxy = listener().await;
    let direct = listener().await;
    let mut command = child(TEST, "1");
    command
        .env(
            "HTTP_PROXY",
            format!("http://user:pass@{}", proxy.local_addr().unwrap()),
        )
        .env("http_proxy", "http://127.0.0.1:1")
        .env("NO_PROXY", "127.0.0.0/8");
    let task = tokio::task::spawn_blocking(move || command.output().unwrap());
    for method in ["PUT", "GET"] {
        let first = receive(
            &proxy,
            "307 Temporary Redirect",
            &format!("Location: {}\r\n", url(&direct)),
            b"",
        )
        .await;
        assert!(first.0.starts_with(&format!(
            "{method} http://source.invalid/pkarr/{} ",
            key().public().to_z32()
        )));
        assert_eq!(
            header(&first.0, "proxy-authorization"),
            Some("Basic dXNlcjpwYXNz")
        );
        let second = receive(
            &direct,
            if method == "PUT" {
                "308 Permanent Redirect"
            } else {
                "303 See Other"
            },
            "Location: http://destination.invalid/record?token=a%2Fb\r\n",
            b"",
        )
        .await;
        assert!(second.0.starts_with(&format!("{method} /pkarr ")));
        assert_eq!(header(&second.0, "proxy-authorization"), None);
        let body = if method == "GET" {
            packet().to_relay_payload()
        } else {
            Vec::new()
        };
        let last = receive(&proxy, "200 OK", "", &body).await;
        assert!(last.0.starts_with(&format!(
            "{method} http://destination.invalid/record?token=a%2Fb "
        )));
        assert_eq!(
            header(&last.0, "proxy-authorization"),
            Some("Basic dXNlcjpwYXNz")
        );
        assert_eq!(first.1, second.1);
        assert_eq!(first.1, last.1);
        if method == "PUT" {
            let signed = SignedPacket::from_relay_payload(&key().public(), &last.1).unwrap();
            assert_eq!(
                EndpointInfo::from_pkarr_signed_packet(&signed).unwrap(),
                info()
            );
        }
    }
    assert_success(timeout(DEADLINE, task).await.unwrap().unwrap());
    assert_idle(&proxy).await;
    assert_idle(&direct).await;
}

#[tokio::test]
async fn shared_bypass_and_proxy_failures_do_not_fall_back() {
    const TEST: &str = "shared_bypass_and_proxy_failures_do_not_fall_back";
    if std::env::var_os(CHILD).is_some() {
        let endpoint = endpoint().await;
        let client = Client::new(std::env::var(URL).unwrap().parse().unwrap(), &endpoint).unwrap();
        let result = client.resolve(key().public()).await;
        if std::env::var(CHILD).unwrap() == "unreachable" {
            assert!(matches!(result, Err(Error::Http(error)) if error.is_connect()));
        } else {
            assert!(matches!(
                result,
                Err(Error::Status(StatusCode::BAD_GATEWAY))
            ));
        }
        endpoint.close().await;
        return;
    }
    for case in [
        "proxy",
        "all-proxy",
        "cidr",
        "wildcard",
        "cgi",
        "unreachable",
    ] {
        let proxy = listener().await;
        let origin = listener().await;
        let mut command = child(TEST, case);
        command
            .env(URL, url(&origin).as_str())
            .env("NO_PROXY", "")
            .env("HTTP_PROXY", url(&proxy).origin().ascii_serialization());
        match case {
            "all-proxy" => {
                command
                    .env("HTTP_PROXY", "invalid://skip-native")
                    .env("ALL_PROXY", url(&proxy).origin().ascii_serialization());
            },
            "cidr" => {
                command.env("NO_PROXY", "127.0.0.0/8");
            },
            "wildcard" => {
                command.env("NO_PROXY", "*");
            },
            "cgi" => {
                command.env("REQUEST_METHOD", "GET");
            },
            _ => {},
        }
        if case == "unreachable" {
            drop(proxy);
            let task = tokio::task::spawn_blocking(move || command.output().unwrap());
            assert_success(timeout(DEADLINE, task).await.unwrap().unwrap());
            assert_idle(&origin).await;
            continue;
        }
        let task = tokio::task::spawn_blocking(move || command.output().unwrap());
        let proxied = matches!(case, "proxy" | "all-proxy");
        let received = receive(
            if proxied { &proxy } else { &origin },
            "502 Bad Gateway",
            "",
            b"",
        )
        .await;
        assert_eq!(received.0.starts_with("GET http://"), proxied);
        assert_success(timeout(DEADLINE, task).await.unwrap().unwrap());
        assert_idle(&proxy).await;
        assert_idle(&origin).await;
    }
}

#[tokio::test]
async fn default_preset_uses_https_proxy_for_publication() {
    const TEST: &str = "default_preset_uses_https_proxy_for_publication";
    if std::env::var_os(CHILD).is_some() {
        let endpoint = Endpoint::builder(crate::DefaultPreset)
            .clear_ip_transports()
            .bind_addr("127.0.0.1:0")
            .unwrap()
            .relay_mode(RelayMode::Disabled)
            .bind()
            .await
            .unwrap();
        assert_eq!(endpoint.address_lookup().unwrap().len(), 2);
        let mut gate = TcpStream::connect(std::env::var(GATE).unwrap())
            .await
            .unwrap();
        timeout(DEADLINE, gate.read_u8()).await.unwrap().unwrap();
        endpoint.close().await;
        return;
    }
    let proxy = listener().await;
    let gate = listener().await;
    let mut command = child(TEST, "1");
    command
        .env("NO_PROXY", "")
        .env("HTTPS_PROXY", url(&proxy).origin().ascii_serialization())
        .env("HTTP_PROXY", "http://127.0.0.1:1")
        .env(GATE, gate.local_addr().unwrap().to_string());
    let task = tokio::task::spawn_blocking(move || command.output().unwrap());
    let received = receive(&proxy, "407 Proxy Authentication Required", "", b"").await;
    assert!(received.0.starts_with("CONNECT dns.iroh.link:443 "));
    let (mut stream, _) = timeout(DEADLINE, gate.accept()).await.unwrap().unwrap();
    stream.write_u8(1).await.unwrap();
    assert_success(timeout(DEADLINE, task).await.unwrap().unwrap());
}

#[tokio::test]
async fn publication_filters_retries_refreshes_and_stops_on_drop() {
    const TEST: &str = "publication_filters_retries_refreshes_and_stops_on_drop";
    if std::env::var_os(CHILD).is_none() {
        assert_success(child(TEST, "1").output().unwrap());
        return;
    }
    let endpoint = endpoint().await;
    let server = listener().await;
    let client = Client::new(url(&server), &endpoint).unwrap();
    let lookup = Lookup::new(client, key(), Duration::from_millis(50));
    let data = info()
        .with_ip_addrs(vec!["127.0.0.1:8080".parse().unwrap()])
        .data;
    lookup.publish(&data);
    let mut timestamp = 0;
    for status in [
        "503 Service Unavailable",
        "204 No Content",
        "204 No Content",
    ] {
        let received = receive(&server, status, "", b"").await;
        let packet = SignedPacket::from_relay_payload(&key().public(), &received.1).unwrap();
        assert_eq!(
            EndpointInfo::from_pkarr_signed_packet(&packet).unwrap(),
            info()
        );
        assert!(packet.timestamp().as_micros() > timestamp);
        timestamp = packet.timestamp().as_micros();
    }
    drop(lookup);
    assert_idle(&server).await;
    endpoint.close().await;
}

#[tokio::test]
async fn invalid_records_and_redirects_are_errors() {
    const TEST: &str = "invalid_records_and_redirects_are_errors";
    if std::env::var_os(CHILD).is_none() {
        assert_success(child(TEST, "1").output().unwrap());
        return;
    }
    let endpoint = endpoint().await;
    for case in [
        "signature",
        "key",
        "size",
        "chunked",
        "location",
        "userinfo",
        "scheme",
        "loop",
        "put",
    ] {
        let server = listener().await;
        let client = Client::new(url(&server), &endpoint).unwrap();
        let task = tokio::spawn(async move {
            if case == "put" {
                client.publish(&packet()).await
            } else {
                client.resolve(key().public()).await.map(|_| ())
            }
        });
        if case == "loop" {
            for _ in 0..11 {
                receive(&server, "302 Found", "Location: /loop\r\n", b"").await;
            }
        } else {
            let (mut stream, _) = timeout(DEADLINE, server.accept()).await.unwrap().unwrap();
            request(&mut stream).await;
            match case {
                "signature" => {
                    let mut body = packet().to_relay_payload();
                    body[0] ^= 1;
                    respond(&mut stream, "200 OK", "", &body).await;
                },
                "key" => {
                    let key = SecretKey::from_bytes(&[24; 32]);
                    let body = EndpointInfo::new(key.public())
                        .to_pkarr_signed_packet(&key, 30)
                        .unwrap()
                        .to_relay_payload();
                    respond(&mut stream, "200 OK", "", &body).await;
                },
                "size" => {
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 99999\r\n\r\n")
                        .await
                        .unwrap();
                },
                "chunked" => {
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n800\r\n")
                        .await
                        .unwrap();
                    stream.write_all(&[0; 2048]).await.unwrap();
                    stream.write_all(b"\r\n0\r\n\r\n").await.unwrap();
                },
                "location" => respond(&mut stream, "302 Found", "", b"").await,
                "userinfo" => {
                    respond(
                        &mut stream,
                        "302 Found",
                        "Location: http://user:secret@127.0.0.1/record\r\n",
                        b"",
                    )
                    .await
                },
                "scheme" => {
                    respond(
                        &mut stream,
                        "302 Found",
                        "Location: ftp://127.0.0.1/record\r\n",
                        b"",
                    )
                    .await
                },
                "put" => respond(&mut stream, "302 Found", "Location: /record\r\n", b"").await,
                _ => unreachable!(),
            }
        }
        let error = timeout(DEADLINE, task).await.unwrap().unwrap().unwrap_err();
        match case {
            "signature" | "key" => assert!(matches!(error, Error::Packet(_))),
            _ => assert!(
                matches!(error, Error::InvalidResponse(_)),
                "{case}: {error}"
            ),
        }
        assert!(!format!("{error:?}").contains("secret"));
        assert_idle(&server).await;
    }
    endpoint.close().await;
}

#[tokio::test]
async fn lookup_stream_returns_verified_data_and_cancels_pending_io() {
    const TEST: &str = "lookup_stream_returns_verified_data_and_cancels_pending_io";
    if std::env::var_os(CHILD).is_none() {
        assert_success(child(TEST, "1").output().unwrap());
        return;
    }
    let endpoint = endpoint().await;
    let server = listener().await;
    let lookup = Lookup::new(
        Client::new(url(&server), &endpoint).unwrap(),
        key(),
        DEFAULT_REPUBLISH_INTERVAL,
    );
    let mut stream = lookup.resolve(key().public()).unwrap();
    let task = tokio::spawn(async move {
        let item = stream.next().await.unwrap().unwrap();
        assert!(stream.next().await.is_none());
        item
    });
    receive(&server, "200 OK", "", &packet().to_relay_payload()).await;
    assert_eq!(task.await.unwrap().endpoint_info(), &info());

    let mut stream = lookup.resolve(key().public()).unwrap();
    let task = tokio::spawn(async move { stream.next().await });
    let (mut connection, _) = timeout(DEADLINE, server.accept()).await.unwrap().unwrap();
    request(&mut connection).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let mut byte = [0];
    assert_eq!(
        timeout(DEADLINE, connection.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    drop(lookup);
    endpoint.close().await;
}

#[tokio::test]
async fn endpoint_tls_roots_are_preserved_and_downgrades_rejected() {
    const TEST: &str = "endpoint_tls_roots_are_preserved_and_downgrades_rejected";
    if std::env::var_os(CHILD).is_none() {
        assert_success(child(TEST, "1").output().unwrap());
        return;
    }
    let cert = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()).into(),
        )
        .unwrap();
    let endpoint = Endpoint::builder(Minimal)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")
        .unwrap()
        .ca_tls_config(iroh::tls::CaTlsConfig::custom_roots([cert
            .cert
            .der()
            .clone()]))
        .bind()
        .await
        .unwrap();
    let server = listener().await;
    let target = listener().await;
    let mut base = url(&server);
    base.set_scheme("https").unwrap();
    let client = Client::new(base, &endpoint).unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
    for downgrade in [false, true] {
        let client = client.clone();
        let task = tokio::spawn(async move { client.resolve(key().public()).await });
        let (stream, _) = timeout(DEADLINE, server.accept()).await.unwrap().unwrap();
        let mut stream = acceptor.accept(stream).await.unwrap();
        request(&mut stream).await;
        if downgrade {
            respond(
                &mut stream,
                "302 Found",
                &format!("Location: {}\r\n", url(&target)),
                b"",
            )
            .await;
        } else {
            let mut location = url(&target);
            location.set_scheme("https").unwrap();
            respond(
                &mut stream,
                "302 Found",
                &format!("Location: {location}\r\n"),
                b"",
            )
            .await;
            let (connection, _) = timeout(DEADLINE, target.accept()).await.unwrap().unwrap();
            let mut stream = acceptor.accept(connection).await.unwrap();
            request(&mut stream).await;
            respond(&mut stream, "200 OK", "", &packet().to_relay_payload()).await;
        }
        let result = timeout(DEADLINE, task).await.unwrap().unwrap();
        if downgrade {
            assert!(matches!(
                result,
                Err(Error::InvalidResponse("HTTPS redirect downgrade"))
            ));
        } else {
            assert_eq!(result.unwrap().endpoint_info(), &info());
        }
    }
    assert_idle(&target).await;
    endpoint.close().await;
}
