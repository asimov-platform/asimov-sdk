// This is free and unencumbered software released into the public domain.

#![cfg(feature = "client")]

extern crate alloc;

use alloc::{format, string::String, vec, vec::Vec};
use asimov_remote::{
    FetcherRequest,
    http::{BatchOptions, OutputFormat, StreamExt},
};
use asimov_social::{
    FetchRequest, FetchRequestOptions, ListRequest, ListRequestOptions, SocialClient,
    SocialClientError, SocialResponseStream,
};
use core::{fmt, time::Duration};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
    time::timeout,
};

const DEADLINE: Duration = Duration::from_secs(5);
const RESPONSE: &[u8] = b"{}\r\n\n\xfftail";

async fn server(endpoint: &'static str, expected: Value) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/api", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        timeout(DEADLINE, async {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            loop {
                let count = socket.read(&mut buffer).await.unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&buffer[..count]);
                let Some(end) = request.windows(4).position(|b| b == b"\r\n\r\n") else {
                    continue;
                };
                let headers = core::str::from_utf8(&request[..end]).unwrap();
                let headers = headers.to_ascii_lowercase();
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                if request.len() < end + 4 + length {
                    continue;
                }
                assert!(headers.starts_with(&format!("post /api/{endpoint} http/1.1\r\n")));
                assert!(headers.contains("\r\nauthorization: bearer test-token"));
                assert!(headers.contains("\r\naccept: application/jsonl"));
                assert_eq!(
                    serde_json::from_slice::<Value>(&request[end + 4..]).unwrap(),
                    expected,
                );
                break;
            }
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/jsonl\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        RESPONSE.len(),
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            socket.write_all(RESPONSE).await.unwrap();
        })
        .await
        .unwrap();
    });
    (url, task)
}

async fn assert_response(mut stream: SocialResponseStream) {
    timeout(DEADLINE, async {
        let mut bytes = Vec::new();
        while let Some(batch) = stream.next().await {
            let batch = batch.unwrap();
            assert_eq!(batch.lines().count(), 1);
            for line in batch.lines() {
                bytes.extend_from_slice(line);
            }
        }
        assert_eq!(bytes, RESPONSE);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn client_forwards_shared_requests_and_preserves_response_bytes() {
    for (endpoint, expected) in [
        ("fetch", json!({"urls": ["example:alice"]})),
        (
            "fetch",
            json!({
                "urls": ["example:alice", "example:bob"],
                "options": {
                    "output": "jsonl", "max-age": "0s", "jev": "",
                    "jq": "select(.name == \"Ada\")", "deadline": "1ns",
                },
            }),
        ),
        ("list", json!({"url": "example:collection"})),
        (
            "list",
            json!({
                "url": "example:collection",
                "options": {"offset": 0, "limit": 0},
            }),
        ),
        (
            "list",
            json!({
                "url": "example:collection",
                "options": {
                    "sort": "-name,handle", "before": "urn:entry:9",
                    "after": "urn:entry:1", "limit": 0, "output": "csv",
                    "other": ["--custom", "literal argument", ""],
                    "max-age": "1ns", "jev": "", "jq": "", "deadline": "0s",
                },
            }),
        ),
    ] {
        let (url, server) = server(endpoint, expected.clone()).await;
        let client = SocialClient::with_base_url("test-token", url)
            .unwrap()
            .with_batching(BatchOptions::new(1, 1024, Duration::from_millis(10)).unwrap());
        let stream = timeout(DEADLINE, async {
            if endpoint == "fetch" {
                // Remote request values are accepted directly by the social API.
                let request: FetcherRequest = serde_json::from_value(expected).unwrap();
                client.fetch(request).await
            } else {
                let request: ListRequest = serde_json::from_value(expected).unwrap();
                client.list(request).await
            }
        })
        .await
        .unwrap()
        .unwrap();
        assert_response(stream).await;
        server.await.unwrap();
    }
}

#[tokio::test]
async fn listing_accepts_display_only_sort_keys_and_formats() {
    #[derive(Clone)]
    struct Name;
    struct Csv;

    impl fmt::Display for Name {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("name")
        }
    }

    impl fmt::Display for Csv {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("csv")
        }
    }

    let (url, server) = server(
        "list",
        json!({
            "url": "example:collection",
            "options": {"sort": "-name", "output": "csv"},
        }),
    )
    .await;
    let client = SocialClient::with_base_url("test-token", url)
        .unwrap()
        .with_batching(BatchOptions::new(1, 1024, Duration::from_millis(10)).unwrap());
    let request = ListRequest::new(
        "example:collection",
        ListRequestOptions::builder()
            .sort(vec![(Name, true).into()].into())
            .output(OutputFormat::Other(Csv))
            .build(),
    );
    let stream = timeout(DEADLINE, client.list(request))
        .await
        .unwrap()
        .unwrap();
    assert_response(stream).await;
    server.await.unwrap();
}

#[tokio::test]
async fn fetching_rejects_unsupported_options_before_sending() {
    let client = SocialClient::with_base_url("test-token", "http://127.0.0.1:1").unwrap();
    for (options, expected) in [
        (
            FetchRequestOptions::builder().output("turtle").build(),
            "output",
        ),
        (
            FetchRequestOptions::builder().other("--custom").build(),
            "other",
        ),
    ] {
        let request = FetchRequest::new("example:alice", options);
        assert!(matches!(
            timeout(DEADLINE, client.fetch(request)).await.unwrap(),
            Err(SocialClientError::UnsupportedOption(actual)) if actual == expected
        ));
    }
}
