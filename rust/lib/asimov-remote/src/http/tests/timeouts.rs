// This is free and unencumbered software released into the public domain.

use super::*;
use crate::http::timeouts as policy;
use asimov_flow::{Bytes, Stream};
use core::{
    future::{Future, pending},
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};
use tokio::{net::TcpStream, sync::mpsc, time::sleep};

const SHORT: Duration = Duration::from_millis(200);

struct StalledDns;

impl reqwest::dns::Resolve for StalledDns {
    fn resolve(&self, _: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(pending())
    }
}

fn executor(url: &str, options: TimeoutOptions) -> Executor {
    let mut executor = Executor::new_with_timeouts(url, Some("token"), options).unwrap();
    executor.client = client_builder(options).unwrap().no_proxy().build().unwrap();
    executor
}

fn assert_timeout(error: &Error, phase: TimeoutPhase) {
    assert!(error.is_timeout(), "{error:?}");
    assert_eq!(error.timeout_phase(), Some(phase), "{error:?}");
}

async fn read_request(socket: &mut TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let count = socket.read(&mut buffer).await.unwrap();
        assert_ne!(count, 0);
        request.extend_from_slice(&buffer[..count]);
        let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
            continue;
        };
        let length = core::str::from_utf8(&request[..end])
            .unwrap()
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .map(str::parse::<usize>)
            })
            .transpose()
            .unwrap()
            .unwrap_or(0);
        if request.len() >= end + 4 + length {
            return request;
        }
    }
}

#[test]
fn rejects_zero_and_overflowing_budgets_before_client_setup() {
    for duration in [Duration::ZERO, Duration::MAX] {
        for phase in [
            TimeoutPhase::Connect,
            TimeoutPhase::ResponseHeaders,
            TimeoutPhase::BodyIdle,
            TimeoutPhase::Discovery,
            TimeoutPhase::Total,
        ] {
            let mut options = TimeoutOptions::disabled();
            match phase {
                TimeoutPhase::Connect => options.connect = Some(duration),
                TimeoutPhase::ResponseHeaders => options.response_headers = Some(duration),
                TimeoutPhase::BodyIdle => options.body_idle = Some(duration),
                TimeoutPhase::Discovery => options.discovery = Some(duration),
                TimeoutPhase::Total => options.total = Some(duration),
            }
            let error =
                Executor::new_with_timeouts("https://example.invalid", Some("token"), options)
                    .unwrap_err();
            assert!(matches!(error, Error::InvalidTimeout(found) if found == phase));
            assert!(!error.is_timeout());
        }
    }
}

#[tokio::test]
async fn connection_budget_covers_dns_tls_connect_and_socks_handshakes() {
    let options = TimeoutOptions {
        connect: Some(SHORT),
        ..TimeoutOptions::disabled()
    };
    let mut client = executor("http://service.invalid", options);
    client.client = client_builder(options)
        .unwrap()
        .no_proxy()
        .dns_resolver(Arc::new(StalledDns))
        .build()
        .unwrap();
    let error = timeout(DEADLINE, client.post_jsonl("fetch", &json!({})))
        .await
        .unwrap()
        .err()
        .unwrap();
    assert_timeout(&error, TimeoutPhase::Connect);

    for scheme in ["https", "http", "socks5h"] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 4096];
            if scheme == "http" {
                assert!(
                    read_request(&mut socket)
                        .await
                        .starts_with(b"CONNECT service.invalid:443 ")
                );
            } else {
                let count = socket.read(&mut buffer).await.unwrap();
                assert_ne!(count, 0);
                assert_eq!(buffer[0], if scheme == "socks5h" { 5 } else { 22 });
            }
            // Never complete the TLS/proxy handshake; cancellation closes it.
            while socket.read(&mut buffer).await.unwrap_or(0) != 0 {}
        });
        let url = if scheme == "https" {
            format!("https://{address}")
        } else {
            String::from("https://service.invalid")
        };
        let mut client = executor(&url, options);
        if scheme != "https" {
            client.client = client_builder(options)
                .unwrap()
                .no_proxy()
                .proxy(reqwest::Proxy::all(format!("{scheme}://{address}")).unwrap())
                .build()
                .unwrap();
        }
        let error = timeout(DEADLINE, client.post_jsonl("fetch", &json!({})))
            .await
            .unwrap()
            .err()
            .unwrap();
        assert_timeout(&error, TimeoutPhase::Connect);
        timeout(DEADLINE, server).await.unwrap().unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn discovery_waiter_keeps_its_original_budget_when_initializer_times_out() {
    let options = TimeoutOptions {
        discovery: Some(SHORT),
        ..TimeoutOptions::disabled()
    };
    let mut client = executor("http://service.invalid", options);
    client.client = client_builder(options)
        .unwrap()
        .no_proxy()
        .dns_resolver(Arc::new(StalledDns))
        .build()
        .unwrap();
    let mut first = Box::pin(client.remote_public_key());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(first.as_mut().poll(&mut cx).is_pending());
    tokio::time::advance(Duration::from_millis(100)).await;
    let mut waiter = Box::pin(client.remote_public_key());
    assert!(waiter.as_mut().poll(&mut cx).is_pending());
    tokio::time::advance(Duration::from_millis(110)).await;
    let Poll::Ready(Err(error)) = first.as_mut().poll(&mut cx) else {
        panic!("initializer did not time out");
    };
    assert_timeout(&error, TimeoutPhase::Discovery);
    assert!(waiter.as_mut().poll(&mut cx).is_pending());
    tokio::time::advance(Duration::from_millis(100)).await;
    let Poll::Ready(Err(error)) = waiter.as_mut().poll(&mut cx) else {
        panic!("waiter's budget was reset by restarting discovery");
    };
    assert_timeout(&error, TimeoutPhase::Discovery);
}

#[tokio::test]
async fn header_timeout_cancels_without_retry_and_executor_can_recover() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        // Partial response headers do not extend the absolute header budget.
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nX-Pending: ")
            .await
            .unwrap();
        let mut buffer = [0; 1024];
        assert_eq!(socket.read(&mut buffer).await.unwrap_or(0), 0);
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n{}\n")
            .await
            .unwrap();
    });
    let options = TimeoutOptions {
        response_headers: Some(SHORT),
        ..TimeoutOptions::disabled()
    };
    let client = executor(&url, options);
    let error = timeout(DEADLINE, client.post_jsonl("fetch", &json!({})))
        .await
        .unwrap()
        .err()
        .unwrap();
    assert_timeout(&error, TimeoutPhase::ResponseHeaders);
    let mut stream = timeout(DEADLINE, client.post_jsonl("fetch", &json!({})))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stream.next().await.unwrap().unwrap().as_contiguous_bytes(),
        Some(b"{}\n".as_slice())
    );
    assert!(stream.next().await.is_none());
    timeout(DEADLINE, server).await.unwrap().unwrap();
}

#[tokio::test]
async fn upload_to_a_nonreading_peer_is_bounded() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (accepted, connected) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (_socket, _) = listener.accept().await.unwrap();
        accepted.send(()).unwrap();
        pending::<()>().await;
    });
    let options = TimeoutOptions {
        response_headers: Some(SHORT),
        ..TimeoutOptions::disabled()
    };
    let client = executor(&url, options);
    let request = json!({ "data": "x".repeat(16 * 1024 * 1024) });
    let error = timeout(DEADLINE, client.post_jsonl("fetch", &request))
        .await
        .unwrap()
        .err()
        .unwrap();
    connected.await.unwrap();
    assert_timeout(&error, TimeoutPhase::ResponseHeaders);
    server.abort();
}

#[tokio::test]
async fn stalled_body_flushes_complete_records_then_one_timeout() {
    let (url, request, resume, server) = http1_server(200, b"{}\npartial", b"").await;
    let options = TimeoutOptions {
        body_idle: Some(SHORT),
        ..TimeoutOptions::disabled()
    };
    let client = executor(&url, options);
    let mut stream = client.post_jsonl("fetch", &json!({})).await.unwrap();
    request.await.unwrap();
    assert_eq!(
        stream.next().await.unwrap().unwrap().as_contiguous_bytes(),
        Some(b"{}\n".as_slice())
    );
    let error = timeout(DEADLINE, stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_timeout(&error, TimeoutPhase::BodyIdle);
    assert!(stream.next().await.is_none());
    drop(resume);
    server.await.unwrap();
}

#[tokio::test]
async fn body_idle_does_not_limit_headers_or_time_before_first_read() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        sleep(SHORT).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n{}\n")
            .await
            .unwrap();
    });
    let options = TimeoutOptions {
        response_headers: Some(Duration::from_secs(2)),
        body_idle: Some(Duration::from_millis(50)),
        ..TimeoutOptions::disabled()
    };
    let client = executor(&url, options);
    let mut stream = timeout(DEADLINE, client.post_jsonl("fetch", &json!({})))
        .await
        .unwrap()
        .unwrap();
    sleep(SHORT).await;
    assert_eq!(
        stream.next().await.unwrap().unwrap().as_contiguous_bytes(),
        Some(b"{}\n".as_slice())
    );
    assert!(stream.next().await.is_none());
    server.await.unwrap();
}

#[tokio::test]
async fn a_progressing_body_outlives_the_header_budget() {
    for total in [None, Some(Duration::from_millis(300))] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                .await
                .unwrap();
            for _ in 0..8 {
                sleep(Duration::from_millis(50)).await;
                if socket.write_all(b"3\r\n{}\n\r\n").await.is_err() {
                    return;
                }
            }
            let _ = socket.write_all(b"0\r\n\r\n").await;
        });
        let options = TimeoutOptions {
            response_headers: Some(SHORT),
            body_idle: Some(SHORT),
            total,
            ..TimeoutOptions::disabled()
        };
        let client = executor(&url, options);
        let mut stream = client.post_jsonl("fetch", &json!({})).await.unwrap();
        let mut count = 0;
        let mut expired = false;
        while let Some(batch) = timeout(DEADLINE, stream.next()).await.unwrap() {
            match batch {
                Ok(batch) => count += batch.len(),
                Err(error) => {
                    assert!(!expired);
                    assert_timeout(&error, TimeoutPhase::Total);
                    expired = true;
                },
            }
        }
        assert_eq!(expired, total.is_some());
        if total.is_some() {
            assert!(count > 0 && count < 8);
        } else {
            assert_eq!(count, 8);
        }
        server.await.unwrap();
    }
}

#[tokio::test]
async fn disabled_header_budget_allows_the_caller_to_keep_waiting() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (resume, gate) = oneshot::channel();
    let (received, request) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        received.send(()).unwrap();
        gate.await.unwrap();
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
    });
    let client = executor(&url, TimeoutOptions::disabled());
    let value = json!({});
    let mut operation = Box::pin(client.post_jsonl("fetch", &value));
    assert!(timeout(SHORT, &mut operation).await.is_err());
    request.await.unwrap();
    resume.send(()).unwrap();
    let mut stream = timeout(DEADLINE, operation).await.unwrap().unwrap();
    assert!(stream.next().await.is_none());
    server.await.unwrap();
}

struct Chunks {
    receiver: mpsc::UnboundedReceiver<Result<Bytes, Error>>,
    dropped: Arc<AtomicBool>,
}

impl Stream for Chunks {
    type Item = Result<Bytes, Error>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

impl Drop for Chunks {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Relaxed);
    }
}

fn chunks() -> (
    mpsc::UnboundedSender<Result<Bytes, Error>>,
    Chunks,
    Arc<AtomicBool>,
) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let dropped = Arc::new(AtomicBool::new(false));
    (
        sender,
        Chunks {
            receiver,
            dropped: dropped.clone(),
        },
        dropped,
    )
}

fn poll(stream: &mut JsonlStream) -> Poll<Option<Result<JsonlBatch, Error>>> {
    stream
        .as_mut()
        .poll_next(&mut Context::from_waker(Waker::noop()))
}

#[tokio::test(start_paused = true)]
async fn empty_chunks_do_not_keep_a_stalled_body_alive() {
    let (sender, source, dropped) = chunks();
    let mut stream = policy::batches(
        source,
        BatchOptions::default(),
        Some(Duration::from_secs(1)),
        None,
    );
    assert!(poll(&mut stream).is_pending());
    for _ in 0..3 {
        tokio::time::advance(Duration::from_millis(300)).await;
        sender.send(Ok(Bytes::new())).unwrap();
        assert!(poll(&mut stream).is_pending());
    }
    tokio::time::advance(Duration::from_millis(200)).await;
    let Poll::Ready(Some(Err(error))) = poll(&mut stream) else {
        panic!("expected idle timeout")
    };
    assert_timeout(&error, TimeoutPhase::BodyIdle);
    assert!(dropped.load(Ordering::Relaxed));
    assert!(matches!(poll(&mut stream), Poll::Ready(None)));
}

#[tokio::test(start_paused = true)]
async fn partial_record_progress_resets_body_inactivity() {
    let (sender, source, dropped) = chunks();
    let mut stream = policy::batches(
        source,
        BatchOptions::default(),
        Some(Duration::from_millis(300)),
        None,
    );
    assert!(poll(&mut stream).is_pending());
    for _ in 0..5 {
        tokio::time::advance(Duration::from_millis(200)).await;
        sender.send(Ok(Bytes::from_static(b"x"))).unwrap();
        assert!(poll(&mut stream).is_pending());
    }
    sender.send(Ok(Bytes::from_static(b"\n"))).unwrap();
    drop(sender);
    let Poll::Ready(Some(Ok(batch))) = poll(&mut stream) else {
        panic!("progressing partial record timed out");
    };
    assert_eq!(batch.as_contiguous_bytes(), Some(b"xxxxx\n".as_slice()));
    assert!(matches!(poll(&mut stream), Poll::Ready(None)));
    assert!(dropped.load(Ordering::Relaxed));
}

#[tokio::test(start_paused = true)]
async fn downstream_pauses_reset_idle_but_not_total_deadlines() {
    for total in [false, true] {
        let (sender, source, dropped) = chunks();
        let deadline =
            policy::Deadline::new(total.then_some(Duration::from_secs(2)), TimeoutPhase::Total)
                .unwrap();
        let mut stream = policy::batches(
            source,
            BatchOptions::default(),
            Some(Duration::from_secs(1)),
            deadline,
        );
        sender.send(Ok(Bytes::from_static(b"{}\npartial"))).unwrap();
        assert!(poll(&mut stream).is_pending());
        tokio::time::advance(Duration::from_millis(20)).await;
        assert!(matches!(poll(&mut stream), Poll::Ready(Some(Ok(batch))) if batch.len() == 1));
        tokio::time::advance(Duration::from_secs(5)).await;
        if total {
            let Poll::Ready(Some(Err(error))) = poll(&mut stream) else {
                panic!("expected total timeout")
            };
            assert_timeout(&error, TimeoutPhase::Total);
            assert!(dropped.load(Ordering::Relaxed));
            assert!(matches!(poll(&mut stream), Poll::Ready(None)));
        } else {
            assert!(poll(&mut stream).is_pending());
            sender.send(Ok(Bytes::from_static(b" tail\n"))).unwrap();
            drop(sender);
            let Poll::Ready(Some(Ok(batch))) = poll(&mut stream) else {
                panic!("expected complete line")
            };
            assert_eq!(
                batch.as_contiguous_bytes(),
                Some(b"partial tail\n".as_slice())
            );
            assert!(matches!(poll(&mut stream), Poll::Ready(None)));
            assert!(dropped.load(Ordering::Relaxed));
        }
    }
}

#[tokio::test]
async fn discovery_total_budget_stops_a_dribbling_body_and_does_not_dispatch() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        assert!(
            read_request(&mut socket)
                .await
                .starts_with(b"GET /.well-known/jwks.json ")
        );
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
            .await
            .unwrap();
        loop {
            if socket.write_all(b"1\r\n \r\n").await.is_err() {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
        assert!(
            timeout(SHORT, listener.accept()).await.is_err(),
            "operation was dispatched after failed discovery"
        );
    });
    let options = TimeoutOptions {
        discovery: Some(SHORT),
        body_idle: Some(Duration::from_secs(1)),
        ..TimeoutOptions::disabled()
    };
    let client = executor(&url, options).with_response_verification();
    let error = timeout(DEADLINE, client.post_jsonl("fetch", &json!({})))
        .await
        .unwrap()
        .err()
        .unwrap();
    assert!(matches!(error, Error::RemoteKey(_)));
    assert_timeout(&error, TimeoutPhase::Discovery);
    timeout(DEADLINE, server).await.unwrap().unwrap();
}
