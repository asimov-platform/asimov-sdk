// This is free and unencumbered software released into the public domain.

use super::*;
use alloc::format;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};

const DEADLINE: Duration = Duration::from_secs(15);
const INDEX: &str = "{\"name\":\"alpha\"}\n{\"name\":\"beta\"}\n";

fn response(status: u16, location: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} Test\r\nlocation: {location}\r\n\
         content-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

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
    .expect("index receiver timed out")
}

fn assert_index(index: Index) {
    let names: Vec<_> = index
        .modules()
        .iter()
        .map(|module| module.name.as_str())
        .collect();
    assert_eq!(names, ["alpha", "beta"]);
}

fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then_some(value.trim())
    })
}

#[tokio::test]
async fn default_fetch_uses_shared_proxy_policy() {
    const CHILD: &str = "ASIMOV_INDEX_HTTP_TEST";
    if let Ok(case) = std::env::var(CHILD) {
        let result = if case == "default-endpoint" {
            Index::fetch().await
        } else {
            Index::fetch_url(&std::env::var("ASIMOV_INDEX_TEST_URL").unwrap()).await
        };
        match case.as_str() {
            "redirect" | "cross-origin-redirect" => assert!(matches!(
                result,
                Err(FetchIndexError::Http(HttpError::NotSuccess(status))) if status == 307
            )),
            "rejected-proxy" => assert!(matches!(
                result,
                Err(FetchIndexError::Http(HttpError::NotSuccess(status))) if status == 407
            )),
            "default-endpoint" => assert!(matches!(
                result,
                Err(FetchIndexError::Http(HttpError::Http(error))) if error.is_connect()
            )),
            "invalid-jsonl" => assert!(matches!(
                result,
                Err(FetchIndexError::Parse(ParseIndexError(2, _)))
            )),
            _ => assert_index(result.unwrap()),
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
        ("default-endpoint", "", 407),
        ("invalid-jsonl", "", 200),
    ] {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/index.jsonl", origin.local_addr().unwrap());
        let proxied = bypass.is_empty();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "index::tests::default_fetch_uses_shared_proxy_policy",
            "--nocapture",
        ]);
        // Snapshot a controlled child environment without unsafe mutation.
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
            .env("ASIMOV_INDEX_TEST_URL", &url)
            .env_remove("REQUEST_METHOD")
            .env("no_proxy", "*")
            .env("NO_PROXY", bypass)
            .env(
                if case == "default-endpoint" {
                    "HTTPS_PROXY"
                } else {
                    "HTTP_PROXY"
                },
                format!("http://user:pass@{}", proxy.local_addr().unwrap()),
            );
        let task = tokio::task::spawn_blocking(move || command.output().unwrap());
        let location = if case == "cross-origin-redirect" {
            "http://redirect.invalid/index.jsonl"
        } else {
            "/redirect.jsonl"
        };
        let body = if case == "invalid-jsonl" {
            "{\"name\":\"alpha\"}\nnot json\n"
        } else {
            INDEX
        };
        let request = receive(
            if proxied { &proxy } else { &origin },
            &response(status, location, body),
        )
        .await;
        if case == "default-endpoint" {
            assert!(request.starts_with("CONNECT raw.githubusercontent.com:443 HTTP/1.1\r\n"));
        } else {
            let target = if proxied { &url } else { "/index.jsonl" };
            assert!(request.starts_with(&format!("GET {target} HTTP/1.1\r\n")));
            assert_eq!(
                header(&request, "user-agent"),
                Some("asimov-module-registry")
            );
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
        // A redirect or proxy failure must not send another request or go direct.
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
async fn fetch_from_uses_caller_client_policy() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/index.jsonl", listener.local_addr().unwrap());
    let client = reqwest::Client::builder()
        .no_proxy()
        .user_agent("caller-policy")
        .redirect(reqwest::redirect::Policy::limited(1))
        .timeout(DEADLINE)
        .build()
        .unwrap();
    let task = tokio::spawn(async move { Index::fetch_from(&client, url).await });
    for (path, status) in [("/index.jsonl", 307), ("/redirect.jsonl", 200)] {
        let request = receive(&listener, &response(status, "/redirect.jsonl", INDEX)).await;
        assert!(request.starts_with(&format!("GET {path} HTTP/1.1\r\n")));
        assert_eq!(header(&request, "user-agent"), Some("caller-policy"));
        assert_eq!(header(&request, "proxy-authorization"), None);
    }
    assert_index(timeout(DEADLINE, task).await.unwrap().unwrap().unwrap());
}

#[tokio::test]
async fn invalid_target_errors_are_returned() {
    for url in ["ftp://index.invalid/index.jsonl", "not a URL"] {
        assert!(matches!(
            Index::fetch_url(url).await,
            Err(FetchIndexError::Proxy(
                asimov_http::proxy::Error::InvalidOrigin(_)
            ))
        ));
    }
}
