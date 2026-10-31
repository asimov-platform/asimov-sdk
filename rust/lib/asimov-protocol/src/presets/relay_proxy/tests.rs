// This is free and unencumbered software released into the public domain.

use super::*;
use alloc::{format, string::String, vec::Vec};
use asimov_http::proxy::ProxyMode;
use core::time::Duration;
use iroh::{Endpoint, RelayMap, RelayMode, RelayUrl, endpoint::presets::Minimal};
use tokio::{
    io::{AsyncBufReadExt as _, BufReader},
    net::TcpListener,
    time::timeout,
};

const RELAYS: [&str; 2] = ["https://relay-a.example/", "https://relay-b.example/"];

fn relays() -> Vec<Url> {
    RELAYS.iter().map(|relay| relay.parse().unwrap()).collect()
}

fn explicit(endpoint: &str, no_proxy: &str) -> ProxyOptions {
    ProxyOptions {
        mode: ProxyMode::Explicit(endpoint.into()),
        no_proxy: Some(no_proxy.into()),
    }
}

#[test]
fn selects_a_shared_http_proxy() {
    for endpoint in [
        "http://user:pass@proxy.example:8080",
        "https://proxy.example",
    ] {
        let selected = select(relays(), &explicit(endpoint, "")).unwrap();
        assert_eq!(selected, parse_endpoint(endpoint));
    }
}

#[test]
fn connects_directly_when_no_proxy_applies_to_every_relay() {
    let cases = [
        (explicit("http://proxy.example", "*"), "bypass all"),
        (
            explicit("http://proxy.example", "relay-a.example"),
            "bypass some",
        ),
        (explicit("socks5h://proxy.example", ""), "SOCKS"),
        (explicit("ftp://proxy.example", ""), "invalid endpoint"),
        (
            ProxyOptions {
                mode: ProxyMode::Direct,
                no_proxy: None,
            },
            "direct mode",
        ),
    ];
    for (options, case) in cases {
        assert_eq!(select(relays(), &options), None, "{case}");
    }
    let options = explicit("http://proxy.example", "");
    assert_eq!(select([], &options), None, "no relays");
}

fn parse_endpoint(endpoint: &str) -> Url {
    asimov_http::proxy::parse_endpoint(endpoint).unwrap()
}

#[tokio::test]
async fn iroh_dials_relays_through_the_selected_proxy_with_credentials() {
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay: Url = "https://relay.example/".parse().unwrap();
    // Reserved characters in credentials are percent-encoded in the URL.
    let endpoint = format!("http://user:p%40ss@{}", proxy.local_addr().unwrap());
    let selected = select([relay.clone()], &explicit(&endpoint, "")).unwrap();
    let endpoint = Endpoint::builder(Minimal)
        .relay_mode(RelayMode::Custom(RelayMap::from(RelayUrl::from(relay))))
        .proxy_url(selected)
        .bind()
        .await
        .unwrap();

    let request = timeout(Duration::from_secs(10), async {
        let (stream, _) = proxy.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            stream.read_line(&mut line).await.unwrap();
            if line.trim_end().is_empty() {
                break lines;
            }
            lines.push(String::from(line.trim_end()));
        }
    })
    .await
    .expect("Iroh should dial the relay through the proxy");
    endpoint.close().await;
    assert!(
        request[0].starts_with("CONNECT relay.example:443 "),
        "{request:?}"
    );
    // RFC 7617 Basic credentials for `user:p@ss`, decoded from the URL.
    let authorization = request.iter().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("proxy-authorization")
            .then(|| value.trim())
    });
    assert_eq!(authorization, Some("Basic dXNlcjpwQHNz"));
}
