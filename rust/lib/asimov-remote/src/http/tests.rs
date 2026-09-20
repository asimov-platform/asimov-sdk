// This is free and unencumbered software released into the public domain.

//! HTTP/1.1 and HTTP/2 transport tests.

use super::*;
use crate::Execute;
use alloc::{vec, vec::Vec};
use core::{convert::Infallible, time::Duration};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
    time::timeout,
};

const DEADLINE: Duration = Duration::from_secs(5);

// A response held open after its first chunk proves the caller receives batches
// before EOF. The server reads and reports the complete JSON request first.
async fn http1_server(
    status: u16,
    first: &'static [u8],
    last: &'static [u8],
) -> (
    String,
    oneshot::Receiver<String>,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/api", listener.local_addr().unwrap());
    let (request_tx, request_rx) = oneshot::channel();
    let (resume_tx, resume_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let count = socket.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0);
            request.extend_from_slice(&buffer[..count]);
            if let Some(end) = request.windows(4).position(|b| b == b"\r\n\r\n") {
                let headers = core::str::from_utf8(&request[..end]).unwrap();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::parse)
                    })
                    .unwrap()
                    .unwrap();
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        request_tx
            .send(String::from_utf8(request).unwrap())
            .unwrap();
        socket.write_all(format!(
            "HTTP/1.1 {status} Test\r\nContent-Type: application/jsonl\r\nTransfer-Encoding: chunked\r\n\r\n"
        ).as_bytes()).await.unwrap();
        if !first.is_empty() {
            socket
                .write_all(format!("{:x}\r\n", first.len()).as_bytes())
                .await
                .unwrap();
            socket.write_all(first).await.unwrap();
            socket.write_all(b"\r\n").await.unwrap();
        }
        if resume_rx.await.is_err() {
            return;
        }
        if !last.is_empty() {
            socket
                .write_all(format!("{:x}\r\n", last.len()).as_bytes())
                .await
                .unwrap();
            socket.write_all(last).await.unwrap();
            socket.write_all(b"\r\n").await.unwrap();
        }
        socket.write_all(b"0\r\n\r\n").await.unwrap();
    });
    (url, request_rx, resume_tx, server)
}

#[tokio::test]
async fn patterns_post_authenticated_json_and_stream_raw_batches_before_eof() {
    async fn fetch(
        operation: &mut impl asimov_patterns::Fetcher<JsonlStream, Error = Error>,
    ) -> JsonlStream {
        operation.execute().await.unwrap()
    }
    async fn list(
        operation: &mut impl asimov_patterns::Lister<JsonlStream, Error = Error>,
    ) -> JsonlStream {
        operation.execute().await.unwrap()
    }
    for endpoint in ["fetch", "list"] {
        let (base_url, request, resume, server) = http1_server(200, b"\n{}\r\n", b"\xfftail").await;
        let executor = Executor::new(format!("{base_url}/"), "test-token").unwrap();
        let mut records = timeout(DEADLINE, async {
            if endpoint == "fetch" {
                fetch(&mut Fetcher::new(
                    executor,
                    "https://example.com/profile",
                    Default::default(),
                ))
                .await
            } else {
                list(&mut Lister::new(
                    executor,
                    "https://example.com/profile",
                    ListerOptions::builder().offset(2).limit(3).build(),
                ))
                .await
            }
        })
        .await
        .unwrap();
        let request = timeout(DEADLINE, request).await.unwrap().unwrap();
        let (headers, body) = request.split_once("\r\n\r\n").unwrap();
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with(&format!("post /api/{endpoint} http/1.1\r\n")));
        assert!(headers.contains("\r\nauthorization: bearer test-token"));
        assert!(headers.contains("\r\naccept: application/jsonl"));
        assert!(headers.contains("\r\ncontent-type: application/json"));
        let expected = if endpoint == "fetch" {
            json!({"urls": ["https://example.com/profile"]})
        } else {
            json!({"url": "https://example.com/profile", "options": {"offset": 2, "limit": 3}})
        };
        assert_eq!(serde_json::from_str::<Value>(body).unwrap(), expected);

        let batch = timeout(DEADLINE, records.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            batch.lines().collect::<Vec<_>>(),
            vec![b"\n".as_slice(), b"{}\r\n"]
        );
        resume.send(()).unwrap();
        let batch = timeout(DEADLINE, records.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            batch.lines().collect::<Vec<_>>(),
            vec![b"\xfftail".as_slice()]
        );
        assert!(records.next().await.is_none());
        server.await.unwrap();
    }
}

#[tokio::test]
async fn http_status_failure_is_a_startup_error() {
    let (url, request, resume, server) = http1_server(401, b"", b"").await;
    let executor = Executor::new(url, "token").unwrap();
    let error = timeout(DEADLINE, executor.post_jsonl("fetch", &json!({})))
        .await
        .unwrap()
        .err()
        .unwrap();
    assert!(
        matches!(error, Error::Http(ref error) if error.status() == Some(reqwest::StatusCode::UNAUTHORIZED))
    );
    request.await.unwrap();
    drop(resume);
    server.await.unwrap();
}

#[tokio::test]
async fn truncated_body_flushes_complete_records_then_reports_failure() {
    let (url, request, resume, server) = http1_server(200, b"{}\npartial", b"").await;
    let executor = Executor::new(url, "token").unwrap();
    let mut batches = executor.post_jsonl("fetch", &json!({})).await.unwrap();
    request.await.unwrap();
    drop(resume); // Close without the terminating HTTP chunk.
    server.await.unwrap();
    let batch = timeout(DEADLINE, batches.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(batch.lines().collect::<Vec<_>>(), vec![b"{}\n".as_slice()]);
    assert!(matches!(batches.next().await, Some(Err(Error::Http(_)))));
    assert!(batches.next().await.is_none());
}

#[tokio::test]
async fn zero_limit_and_unsupported_options_do_not_start_requests() {
    let executor = Executor::new("http://127.0.0.1:1", "token").unwrap();
    let mut operation = Lister::new(
        executor.clone(),
        "https://example.com",
        ListerOptions::builder().limit(0).build(),
    );
    assert!(operation.execute().await.unwrap().next().await.is_none());
    assert_eq!(operation.capabilities().limit, OptionSupport::Supported);
    assert_eq!(operation.capabilities().after, OptionSupport::Unsupported);
    for options in [
        ListerOptions::builder()
            .limit(0)
            .after("urn:entry:1")
            .build(),
        ListerOptions::builder().before("urn:entry:2").build(),
        ListerOptions::builder()
            .sort("name".parse().unwrap())
            .build(),
        ListerOptions::builder().other("--custom").build(),
        ListerOptions::builder().output("turtle").build(),
    ] {
        let mut operation = Lister::new(executor.clone(), "https://example.com", options);
        assert!(matches!(
            operation.execute().await,
            Err(Error::UnsupportedOption(_))
        ));
    }
    let mut operation = Fetcher::new(
        executor,
        "https://example.com",
        FetcherOptions::builder().output("turtle").build(),
    );
    assert!(matches!(
        operation.execute().await,
        Err(Error::UnsupportedOption("output"))
    ));
}

#[test]
fn configuration_supports_prefixes_and_redacts_token() {
    let executor =
        Executor::new("https://example.com/api?ignored=1#ignored", "secret-token").unwrap();
    assert_eq!(executor.base_url().as_str(), "https://example.com/api/");
    assert!(!format!("{executor:?}").contains("secret-token"));
    for url in ["invalid", "file:///tmp/example", "mailto:me@example.com"] {
        assert!(Executor::new(url, "token").is_err());
    }
}

#[tokio::test]
async fn http2_requests_can_reuse_the_same_operation() {
    // Reuse the same configured operation against two requests on one service.
    use asimov_flow::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper::{Response, service::service_fn};
    use hyper_util::rt::{TokioExecutor, TokioIo};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut executor = Executor::new(
        format!("http://{}", listener.local_addr().unwrap()),
        "token",
    )
    .unwrap();
    // h2c isolates HTTP/2 framing from TLS certificate provisioning in this test.
    // Production uses the executor's rustls/ALPN configuration above.
    executor.client = Client::builder().http2_prior_knowledge().build().unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        hyper::server::conn::http2::Builder::new(TokioExecutor::new())
            .serve_connection(
                TokioIo::new(socket),
                service_fn(
                    |request: hyper::Request<hyper::body::Incoming>| async move {
                        assert_eq!(request.version(), hyper::Version::HTTP_2);
                        assert_eq!(request.uri().path(), "/fetch");
                        assert_eq!(request.headers()["authorization"], "Bearer token");
                        let bytes = request.into_body().collect().await.unwrap().to_bytes();
                        assert_eq!(
                            serde_json::from_slice::<Value>(&bytes).unwrap(),
                            json!({"urls":["https://example.com"]})
                        );
                        Ok::<_, Infallible>(
                            Response::builder()
                                .header("content-type", "application/jsonl")
                                .body(Full::new(Bytes::from_static(b"{}\n[]\n")))
                                .unwrap(),
                        )
                    },
                ),
            )
            .await
            .unwrap();
    });
    let mut operation = Fetcher::new(executor, "https://example.com", Default::default());
    for _ in 0..2 {
        let mut stream = timeout(DEADLINE, operation.execute())
            .await
            .unwrap()
            .unwrap();
        let mut bytes = Vec::new();
        while let Some(batch) = timeout(DEADLINE, stream.next()).await.unwrap() {
            for line in batch.unwrap().lines() {
                bytes.extend_from_slice(line);
            }
        }
        assert_eq!(bytes, b"{}\n[]\n");
    }
    drop(operation);
    server.abort();
    let _ = server.await;
}
