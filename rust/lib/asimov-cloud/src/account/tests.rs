// This is free and unencumbered software released into the public domain.

extern crate std;

use super::*;
use alloc::string::String;
use core::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};

const DEADLINE: Duration = Duration::from_secs(15);

async fn receive(listener: &TcpListener, response: &str) -> String {
    timeout(DEADLINE, async {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = tokio::io::BufReader::new(stream);
        let mut request = String::new();
        while !request.ends_with("\r\n\r\n") {
            assert!(stream.read_line(&mut request).await.unwrap() > 0);
            assert!(request.len() < 8192);
        }
        stream
            .get_mut()
            .write_all(response.as_bytes())
            .await
            .unwrap();
        request
    })
    .await
    .expect("balance receiver timed out")
}

fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then_some(value.trim())
    })
}

#[tokio::test]
async fn balances_use_shared_proxy_policy() {
    const CHILD: &str = "ASIMOV_CLOUD_HTTP_TEST";
    if let Ok(case) = std::env::var(CHILD) {
        let account: Account = "alice".parse().unwrap();
        let result = timeout(DEADLINE, async {
            match case.as_str() {
                "balance-endpoint" => account.balance().await,
                "has-credits-endpoint" => account.has_credits().await.map(|has_credits| {
                    if has_credits {
                        Credits::ONE
                    } else {
                        Credits::ZERO
                    }
                }),
                _ => Account::fetch_balance(&std::env::var("ASIMOV_CLOUD_TEST_URL").unwrap()).await,
            }
        })
        .await
        .expect("balance request timed out");
        match case.as_str() {
            "redirect" | "cross-origin-redirect" => assert!(matches!(
                result,
                Err(AccountBalanceError::UnexpectedResponse(status)) if status == 307
            )),
            "rejected-proxy" => assert!(matches!(
                result,
                Err(AccountBalanceError::UnexpectedResponse(status)) if status == 407
            )),
            "non-ok-success" => assert!(matches!(
                result,
                Err(AccountBalanceError::UnexpectedResponse(status)) if status == 204
            )),
            "balance-endpoint" | "has-credits-endpoint" => assert!(matches!(
                result,
                Err(AccountBalanceError::FailedRequest(error)) if error.is_connect()
            )),
            "invalid-balance" => assert!(matches!(
                result,
                Err(AccountBalanceError::InvalidResponseFormat(_))
            )),
            _ => assert_eq!(result.unwrap(), Credits::from_nanos(12_500_000_000)),
        }
        return;
    }

    for (case, bypass, status) in [
        ("proxy", "", 200),
        ("host-bypass", "127.0.0.1", 200),
        ("wildcard-bypass", "*", 200),
        ("redirect", "", 307),
        ("cross-origin-redirect", "", 307),
        ("rejected-proxy", "", 407),
        ("balance-endpoint", "", 407),
        ("has-credits-endpoint", "", 407),
        ("non-ok-success", "", 204),
        ("invalid-balance", "", 200),
    ] {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/alice/balance.txt", origin.local_addr().unwrap());
        let proxied = bypass.is_empty();
        let public_endpoint = matches!(case, "balance-endpoint" | "has-credits-endpoint");
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "account::tests::balances_use_shared_proxy_policy",
            "--nocapture",
        ]);
        // Isolate discovery without changing the multithreaded test environment.
        for name in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            command.env(name, "http://127.0.0.1:1");
        }
        command
            .env(CHILD, case)
            .env("ASIMOV_CLOUD_TEST_URL", &url)
            .env_remove("REQUEST_METHOD")
            .env("no_proxy", "*")
            .env("NO_PROXY", bypass)
            .env(
                if public_endpoint {
                    "HTTPS_PROXY"
                } else {
                    "HTTP_PROXY"
                },
                format!("http://user:pass@{}", proxy.local_addr().unwrap()),
            );
        let task = tokio::task::spawn_blocking(move || command.output().unwrap());
        let location = if case == "cross-origin-redirect" {
            "http://redirect.invalid/balance.txt"
        } else {
            "/redirect.txt"
        };
        let body = match case {
            "invalid-balance" => "not a balance",
            "non-ok-success" => "",
            _ => "12.500000000",
        };
        let response = format!(
            "HTTP/1.1 {status} Test\r\nlocation: {location}\r\n\
             content-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let request = receive(if proxied { &proxy } else { &origin }, &response).await;
        if public_endpoint {
            assert!(request.starts_with("CONNECT asimov.credit:443 HTTP/1.1\r\n"));
        } else {
            let target = if proxied { &url } else { "/alice/balance.txt" };
            assert!(request.starts_with(&format!("GET {target} HTTP/1.1\r\n")));
            assert_eq!(header(&request, "user-agent"), Some("ASIMOV.rs"));
        }
        assert_eq!(
            header(&request, "proxy-authorization"),
            proxied.then_some("Basic dXNlcjpwYXNz"),
            "{case}"
        );
        let output = timeout(DEADLINE, task).await.unwrap().unwrap();
        assert!(
            output.status.success(),
            "{case}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        // No redirected request or direct fallback after proxy failure.
        for listener in [&origin, &proxy] {
            assert!(
                timeout(Duration::from_millis(50), listener.accept())
                    .await
                    .is_err(),
                "unexpected connection: {case}"
            );
        }
    }
}

#[tokio::test]
async fn invalid_native_targets_return_proxy_errors() {
    for url in ["ftp://account.invalid/balance.txt", "not a URL"] {
        assert!(matches!(
            Account::fetch_balance(url).await,
            Err(AccountBalanceError::Proxy(
                asimov_http::proxy::Error::InvalidOrigin(_)
            ))
        ));
    }
}
