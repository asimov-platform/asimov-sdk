// This is free and unencumbered software released into the public domain.

use super::*;
use crate::installer::{Installer, test_support::*};
use alloc::format;
use asimov_registry::Registry;
use tokio::{io::AsyncWriteExt, net::TcpListener, time::timeout};

#[tokio::test]
async fn redirects_resolve_each_destination_for_get_and_head() {
    if let Ok(method) = std::env::var(CHILD) {
        let root = tempfile::tempdir().unwrap();
        let installer = Installer::with_registry(Registry::new(root.path(), Default::default()));
        let url = "http://source.invalid/latest";
        let response = timeout(DEADLINE, async {
            if method == "HEAD" {
                installer.client.head(url).await
            } else {
                installer.client.get(url).await
            }
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.url().as_str(),
            "http://cdn.invalid/asset?token=a%2Fb"
        );
        let body = response.bytes().await.unwrap();
        assert_eq!(
            body.as_ref(),
            if method == "HEAD" {
                b"".as_slice()
            } else {
                b"asset"
            }
        );
        return;
    }
    for method in ["GET", "HEAD"] {
        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let direct = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut command = child(
            "http::tests::redirects_resolve_each_destination_for_get_and_head",
            method,
        );
        command
            .env(
                "HTTP_PROXY",
                format!("http://user:pass@{}", proxy.local_addr().unwrap()),
            )
            .env("NO_PROXY", "127.0.0.1");
        let task = tokio::task::spawn_blocking(move || command.output().unwrap());
        let first = receive(
            &proxy,
            &format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{}/hop\r\nContent-Length: 0\r\n\r\n",
                direct.local_addr().unwrap()
            ),
        )
        .await;
        assert!(first.starts_with(&format!(
            "{method} http://source.invalid/latest HTTP/1.1\r\n"
        )));
        assert_eq!(
            header(&first, "proxy-authorization"),
            Some("Basic dXNlcjpwYXNz")
        );
        let second = receive(&direct,
            "HTTP/1.1 303 See Other\r\nLocation: http://cdn.invalid/asset?token=a%2Fb\r\nContent-Length: 0\r\n\r\n"
        ).await;
        assert!(second.starts_with(&format!("{method} /hop HTTP/1.1\r\n")));
        assert_eq!(header(&second, "proxy-authorization"), None);
        let last = receive(
            &proxy,
            &format!(
                "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n{}",
                if method == "HEAD" { "" } else { "asset" }
            ),
        )
        .await;
        assert!(last.starts_with(&format!(
            "{method} http://cdn.invalid/asset?token=a%2Fb HTTP/1.1\r\n"
        )));
        assert_eq!(
            header(&last, "proxy-authorization"),
            Some("Basic dXNlcjpwYXNz")
        );
        assert_eq!(header(&last, "user-agent"), Some("asimov-module-installer"));
        assert_success(timeout(DEADLINE, task).await.unwrap().unwrap());
        assert_idle(&proxy).await;
        assert_idle(&direct).await;
    }
}

#[tokio::test]
async fn automatic_clients_reuse_connections_across_clones() {
    if std::env::var_os(CHILD).is_none() {
        assert_success(
            child(
                "http::tests::automatic_clients_reuse_connections_across_clones",
                "1",
            )
            .output()
            .unwrap(),
        );
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/asset", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        for _ in 0..2 {
            assert!(
                request(&mut stream)
                    .await
                    .starts_with("GET /asset HTTP/1.1\r\n")
            );
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await
                .unwrap();
        }
        assert_idle(&listener).await;
    });
    let client = Client::default();
    let clone = client.clone();
    timeout(DEADLINE, async {
        for client in [client, clone] {
            assert_eq!(client.get(&url).await.unwrap().bytes().await.unwrap(), "ok");
        }
        server.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn redirect_failures_and_proxy_errors_are_reported() {
    if std::env::var_os(CHILD).is_none() {
        assert_success(
            child(
                "http::tests::redirect_failures_and_proxy_errors_are_reported",
                "1",
            )
            .output()
            .unwrap(),
        );
        return;
    }
    for (location, reason) in [
        ("", "missing Location header"),
        ("Location: http://[invalid\r\n", "invalid target URL"),
        ("Location: /loop\r\n", "too many redirects"),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/loop", listener.local_addr().unwrap());
        let client = Client::default();
        let task = tokio::spawn(async move { client.get(&url).await });
        let count = if reason == "too many redirects" {
            MAX_REDIRECTS + 1
        } else {
            1
        };
        for _ in 0..count {
            receive(&listener, &format!(
                "HTTP/1.1 307 Temporary Redirect\r\n{location}Content-Length: 0\r\nConnection: close\r\n\r\n"
            )).await;
        }
        assert!(matches!(timeout(DEADLINE, task).await.unwrap().unwrap(),
            Err(HttpError::Redirect(message)) if message == reason));
        assert_idle(&listener).await;
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/invalid", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { Client::default().get(&url).await });
    receive(
        &listener,
        "HTTP/1.1 302 Found\r\nLocation: ftp://service.invalid/asset\r\nContent-Length: 0\r\n\r\n",
    )
    .await;
    assert!(matches!(
        timeout(DEADLINE, task).await.unwrap().unwrap(),
        Err(HttpError::Proxy(asimov_http::proxy::Error::InvalidOrigin(
            _
        )))
    ));
    assert_idle(&listener).await;
}

#[tokio::test]
async fn public_release_lookup_uses_https_proxy() {
    if std::env::var_os(CHILD).is_some() {
        let root = tempfile::tempdir().unwrap();
        let installer = Installer::with_registry(Registry::new(root.path(), Default::default()));
        let result = timeout(
            DEADLINE,
            installer.fetch_latest_release(&"example".parse().unwrap()),
        )
        .await
        .unwrap();
        assert!(
            matches!(result, Err(crate::installer::error::FetchError::Http(HttpError::Http(error)))
            if error.is_connect())
        );
        return;
    }
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut command = child("http::tests::public_release_lookup_uses_https_proxy", "1");
    command
        .env(
            "HTTPS_PROXY",
            format!("http://{}", proxy.local_addr().unwrap()),
        )
        .env("NO_PROXY", "");
    let task = tokio::task::spawn_blocking(move || command.output().unwrap());
    let request = receive(
        &proxy,
        "HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n",
    )
    .await;
    assert!(request.starts_with("CONNECT github.com:443 HTTP/1.1\r\n"));
    assert_success(timeout(DEADLINE, task).await.unwrap().unwrap());
    assert_idle(&proxy).await;
}

#[tokio::test]
async fn supplied_clients_own_transport_policy() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/latest", listener.local_addr().unwrap());
    let raw = reqwest::Client::builder()
        .no_proxy()
        .user_agent("caller-policy")
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let installer = Installer::new(raw, Registry::new(root.path(), Default::default()));
    let task = tokio::spawn(async move { installer.client.get(&url).await });
    let request = receive(
        &listener,
        "HTTP/1.1 302 Found\r\nLocation: /elsewhere\r\nContent-Length: 0\r\n\r\n",
    )
    .await;
    assert_eq!(header(&request, "user-agent"), Some("caller-policy"));
    assert_eq!(
        timeout(DEADLINE, task)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .status(),
        302
    );
    assert_idle(&listener).await;
}
