// This is free and unencumbered software released into the public domain.

//! OpenAI-compatible proxy support, currently forwarding to OpenRouter.
//!
//! ```no_run
//! # extern crate alloc;
//! # async fn example() -> Result<(), alloc::boxed::Box<dyn core::error::Error>> {
//! use asimov_proxy::openai::{Proxy, ProxyOptions};
//!
//! let proxy = Proxy::openrouter("my-api-key", ProxyOptions::default())?;
//! let listener = tokio::net::TcpListener::bind("127.0.0.1:1920").await?;
//! // Replace this future with the application's shutdown notification.
//! proxy.serve(listener, core::future::pending()).await?;
//! # Ok(())
//! # }
//! ```

use crate::{BodyLogger, Error, ProxyConfig, proxy_connector::ProxyConnector};
use alloc::{format, string::String, sync::Arc, vec::Vec};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Request, State},
    http::{self, HeaderMap, HeaderValue, StatusCode, Version},
    response::Response,
    routing::any,
};
use core::time::Duration;
use http_body_util::{BodyExt, Full};
use hyper_rustls::{ConfigBuilderExt as _, HttpsConnector};
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use tokio::net::TcpListener;

const UPSTREAM_BASE_URL: &str = "https://openrouter.ai/api";
const DEFAULT_MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// The upstream HTTP client: a hyper client speaking rustls-based TLS to the
/// target, over a connection that is either direct or tunneled through a
/// proxy (see the `proxy_connector` module).
type UpstreamClient = Client<HttpsConnector<ProxyConnector>, Full<Bytes>>;

/// Transport, resource limits, and optional logging for a [`Proxy`].
#[derive(Clone)]
pub struct ProxyOptions {
    /// How to connect upstream; defaults to a direct connection.
    pub upstream_proxy: ProxyConfig,
    /// Maximum buffered request body size; defaults to 16 MiB.
    pub max_body_bytes: usize,
    /// Deadline for upstream response headers; defaults to 120 seconds.
    ///
    /// Includes connection establishment, but does not limit response streaming.
    pub upstream_timeout: Duration,
    /// Optional request and response body log; disabled by default.
    pub logger: Option<BodyLogger>,
}

impl Default for ProxyOptions {
    fn default() -> Self {
        Self {
            upstream_proxy: ProxyConfig::Direct,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            upstream_timeout: Duration::from_secs(120),
            logger: None,
        }
    }
}

#[derive(Clone)]
struct ProxyState {
    client: UpstreamClient,
    upstream_base_url: String,
    logger: Option<BodyLogger>,
    authorization: HeaderValue,
    max_body_bytes: usize,
    upstream_timeout: Duration,
}

/// An OpenAI-compatible proxy forwarding requests to OpenRouter.
///
/// Requests retain their path, query, method, and body. The upstream API key
/// replaces inbound authorization, and ASIMOV attribution headers are added.
/// Hop-by-hop headers are stripped in both directions. Responses, including
/// SSE, are streamed without buffering.
#[derive(Clone)]
pub struct Proxy {
    state: ProxyState,
}

impl Proxy {
    /// Creates an OpenRouter proxy with explicit credentials and options.
    ///
    /// Loads native TLS roots and validates the API key. Does not read proxy
    /// environment variables; use [`ProxyConfig::from_env`] to opt in.
    pub fn openrouter(api_key: &str, options: ProxyOptions) -> Result<Self, Error> {
        let authorization = authorization_header(api_key)?;

        // Select a provider explicitly, even when downstream dependencies also
        // enable rustls's aws-lc provider.
        let tls_config = Arc::new(
            rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()?
            .with_native_roots()?
            .with_no_client_auth(),
        );
        let connector = ProxyConnector::new(options.upstream_proxy, Arc::clone(&tls_config));
        let https_connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config((*tls_config).clone())
            .https_only()
            .enable_http1()
            .wrap_connector(connector);
        let client = Client::builder(TokioExecutor::new()).build(https_connector);

        Ok(Self {
            state: ProxyState {
                client,
                upstream_base_url: UPSTREAM_BASE_URL.into(),
                logger: options.logger,
                authorization,
                max_body_bytes: options.max_body_bytes,
                upstream_timeout: options.upstream_timeout,
            },
        })
    }

    /// Creates a router for embedding in an existing Axum application.
    ///
    /// Invalid bodies return HTTP 400, oversized bodies 413, upstream failures
    /// 502, and upstream header timeouts 504. Upstream status codes pass through.
    pub fn router(self) -> Router {
        Router::new()
            .route("/{*path}", any(proxy_handler))
            .with_state(self.state)
    }

    /// Serves a caller-owned listener until `shutdown` completes.
    ///
    /// Stops accepting connections on shutdown and waits for in-flight
    /// responses to finish. The caller controls signals and shutdown deadlines.
    pub async fn serve(
        self,
        listener: TcpListener,
        shutdown: impl core::future::Future<Output = ()> + Send + 'static,
    ) -> std::io::Result<()> {
        serve_until_shutdown(listener, self.router(), shutdown).await
    }
}

async fn serve_until_shutdown(
    listener: TcpListener,
    router: Router,
    shutdown: impl core::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}

async fn proxy_handler(
    State(state): State<ProxyState>,
    req: Request,
) -> Result<Response, StatusCode> {
    let request_path = req.uri().path();
    let request_query = req
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();

    #[cfg(feature = "tracing")]
    tracing::info!("Proxying request: {} {}", request_path, request_query);

    // https://openrouter.ai/api/v1/chat/completions
    let target_url = format!(
        "{}{}{}",
        state.upstream_base_url, request_path, request_query
    );

    let (mut head, body) = req.into_parts();

    let upstream_request_body = read_request_body(body, state.max_body_bytes).await?;

    if let Some(logger) = &state.logger {
        logger.log_request_body(&upstream_request_body);
    }

    // Retarget the request at the upstream server:
    head.uri = target_url
        .parse()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    head.version = Version::HTTP_11; // regardless of the inbound HTTP version

    // Modify request headers:
    strip_hop_by_hop_headers(&mut head.headers);
    head.headers.remove("host"); // don't send "Host: 127.0.0.1"
    head.headers.remove("content-length"); // hyper recomputes the buffered length
    head.headers
        .insert("Authorization", state.authorization.clone());

    // See: https://openrouter.ai/docs/app-attribution
    insert_attribution_headers(&mut head.headers);

    let upstream_request = http::Request::from_parts(head, Full::new(upstream_request_body));

    let upstream_response = wait_for_upstream(
        state.upstream_timeout,
        state.client.request(upstream_request),
    )
    .await?;

    Ok(forward_response(upstream_response, state.logger.clone()))
}

fn forward_response(
    response: http::Response<hyper::body::Incoming>,
    logger: Option<BodyLogger>,
) -> Response {
    // Stream the upstream response body back to the client, teeing each data
    // frame into the body log (if enabled):
    let (mut head, upstream_response_body) = response.into_parts();
    strip_hop_by_hop_headers(&mut head.headers);
    let upstream_response_body = upstream_response_body.map_frame(move |frame| {
        if let (Some(logger), Some(data)) = (&logger, frame.data_ref()) {
            logger.log_response_chunk(data);
        }
        frame
    });

    Response::from_parts(head, Body::new(upstream_response_body))
}

fn strip_hop_by_hop_headers(headers: &mut HeaderMap) {
    let nominated: Vec<_> = headers
        .get_all(http::header::CONNECTION)
        .iter()
        .flat_map(|value| value.as_bytes().split(|byte| *byte == b','))
        .filter_map(|name| http::header::HeaderName::from_bytes(name.trim_ascii()).ok())
        .collect();
    for name in nominated {
        headers.remove(name);
    }
    for name in [
        "connection",
        "keep-alive",
        "proxy-connection",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ] {
        headers.remove(name);
    }
}

async fn wait_for_upstream<T, E: core::fmt::Display>(
    limit: Duration,
    request: impl core::future::Future<Output = Result<T, E>>,
) -> Result<T, StatusCode> {
    match tokio::time::timeout(limit, request).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(_error)) => {
            #[cfg(feature = "tracing")]
            tracing::error!("Upstream request failed: {_error}");
            Err(StatusCode::BAD_GATEWAY)
        },
        Err(_) => {
            #[cfg(feature = "tracing")]
            tracing::error!("Upstream response headers timed out");
            Err(StatusCode::GATEWAY_TIMEOUT)
        },
    }
}

async fn read_request_body(body: Body, limit: usize) -> Result<Bytes, StatusCode> {
    use core::error::Error;
    axum::body::to_bytes(body, limit).await.map_err(|error| {
        if error
            .source()
            .is_some_and(|cause| cause.is::<http_body_util::LengthLimitError>())
        {
            StatusCode::PAYLOAD_TOO_LARGE
        } else {
            StatusCode::BAD_REQUEST
        }
    })
}

fn authorization_header(api_key: &str) -> Result<HeaderValue, Error> {
    if api_key.trim().is_empty() {
        return Err(Error::MissingApiKey);
    }
    let mut header =
        HeaderValue::from_str(&format!("Bearer {api_key}")).map_err(|_| Error::InvalidApiKey)?;
    header.set_sensitive(true);
    Ok(header)
}

fn insert_attribution_headers(headers: &mut HeaderMap<HeaderValue>) {
    // See: https://openrouter.ai/docs/app-attribution
    headers.insert(
        "HTTP-Referer",
        HeaderValue::from_static("https://asimov.sh"),
    );
    headers.insert("X-OpenRouter-Title", HeaderValue::from_static("ASIMOV"));
    headers.insert(
        "X-OpenRouter-Categories",
        HeaderValue::from_static("cli-agent,personal-agent"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn serves_requests_with_upstream_credentials_and_streamed_logged_responses() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let (release, released) = tokio::sync::oneshot::channel();
        let body = b"{\"model\": \"openrouter/free\", \"stream\": true}";
        let upstream_task = tokio::spawn(async move {
            let (mut socket, _) = upstream.accept().await.unwrap();
            let headers = read_request_headers(&mut socket).await.to_ascii_lowercase();
            assert!(headers.starts_with("post /api/v1/chat/completions?test=1 http/1.1\r\n"));
            for header in [
                format!("host: {upstream_address}\r\n"),
                format!("content-length: {}\r\n", body.len()),
                "authorization: bearer test-key\r\n".into(),
                "http-referer: https://asimov.sh\r\n".into(),
                "x-openrouter-title: asimov\r\n".into(),
                "x-openrouter-categories: cli-agent,personal-agent\r\n".into(),
                "content-type: application/json\r\n".into(),
            ] {
                assert!(headers.contains(&header), "{headers}");
            }
            for removed in [
                "inbound-key",
                "x-private:",
                "proxy-authorization:",
                "connection:",
            ] {
                assert!(!headers.contains(removed), "{headers}");
            }
            let mut received_body = alloc::vec![0; body.len()];
            socket.read_exact(&mut received_body).await.unwrap();
            assert_eq!(received_body, body);

            socket.write_all(b"HTTP/1.1 201 Created\r\nContent-Type: text/event-stream\r\nConnection: keep-alive, x-private\r\nX-Private: secret\r\nTransfer-Encoding: chunked\r\n\r\nd\r\ndata: first\n\n\r\n").await.unwrap();
            released.await.unwrap();
            socket
                .write_all(b"e\r\ndata: second\n\n\r\n0\r\n\r\n")
                .await
                .unwrap();
            drop(socket);

            // Close the next request without a response to trigger HTTP 502.
            // Connecting to a closed port can outlast the deadline on Windows.
            let (mut socket, _) = upstream.accept().await.unwrap();
            read_request_headers(&mut socket).await;
        });

        // Use a local cleartext upstream to exercise the entire serving path.
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
        let connector = ProxyConnector::new(ProxyConfig::Direct, Arc::new(tls.clone()));
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(tls)
            .https_or_http()
            .enable_http1()
            .wrap_connector(connector);
        let root = tempfile::tempdir().unwrap();
        let log_path = root.path().join("bodies.log");
        let proxy = Proxy {
            state: ProxyState {
                client: Client::builder(TokioExecutor::new()).build(connector),
                upstream_base_url: format!("http://{upstream_address}/api"),
                authorization: authorization_header("test-key").unwrap(),
                logger: Some(BodyLogger::open(&log_path).unwrap()),
                max_body_bytes: body.len(),
                upstream_timeout: Duration::from_secs(2),
            },
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(proxy.serve(listener, async {
            let _ = stopped.await;
        }));
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let url = format!("http://{address}/v1/chat/completions?test=1");

        // Rejected uploads must not establish an upstream connection.
        let oversized = client
            .post(&url)
            .body(alloc::vec![0; body.len() + 1])
            .send()
            .await
            .unwrap();
        assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let mut response = client
            .post(&url)
            .header("authorization", "Bearer inbound-key")
            .header("proxy-authorization", "Basic inbound-key")
            .header("connection", "x-private")
            .header("x-private", "secret")
            .header("host", "inbound.example")
            .header("content-type", "application/json")
            .body(body.as_slice())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        assert!(!response.headers().contains_key("x-private"));
        let first = response.chunk().await.unwrap().unwrap();
        assert_eq!(first, "data: first\n\n");
        let partial_log = std::fs::read_to_string(&log_path).unwrap();
        assert!(partial_log.contains(core::str::from_utf8(body).unwrap()));
        assert!(partial_log.contains("data: first"));
        assert!(!partial_log.contains("data: second"));
        release.send(()).unwrap();
        assert_eq!(response.text().await.unwrap(), "data: second\n\n");
        assert!(
            std::fs::read_to_string(log_path)
                .unwrap()
                .contains("data: second")
        );

        // The upstream closes without responding; errors become HTTP 502.
        let failed = client.get(&url).send().await.unwrap();
        assert_eq!(failed.status(), StatusCode::BAD_GATEWAY);
        upstream_task.await.unwrap();
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[test]
    fn removes_standard_and_connection_nominated_headers() {
        let mut headers = HeaderMap::new();
        for name in [
            "keep-alive",
            "proxy-connection",
            "proxy-authenticate",
            "proxy-authorization",
            "te",
            "trailer",
            "transfer-encoding",
            "upgrade",
            "x-first",
            "x-second",
        ] {
            headers.insert(
                http::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_static("value"),
            );
        }
        headers.append(
            "connection",
            HeaderValue::from_static("keep-alive, X-First"),
        );
        headers.append(
            "connection",
            HeaderValue::from_static(" X-Second , invalid name"),
        );
        headers.insert(
            "content-type",
            HeaderValue::from_static("text/event-stream"),
        );
        headers.insert("x-end-to-end", HeaderValue::from_static("keep"));
        strip_hop_by_hop_headers(&mut headers);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers["content-type"], "text/event-stream");
        assert_eq!(headers["x-end-to-end"], "keep");
    }

    #[tokio::test]
    async fn sanitized_chunked_responses_remain_streaming() {
        use tokio::io::AsyncWriteExt;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release, released) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request_headers(&mut socket).await;
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive, x-private\r\nX-Private: secret\r\nProxy-Authenticate: Basic\r\nTransfer-Encoding: chunked\r\n\r\nd\r\ndata: first\n\n\r\n").await.unwrap();
            released.await.unwrap();
            socket
                .write_all(b"e\r\ndata: second\n\n\r\n0\r\n\r\n")
                .await
                .unwrap();
        });
        let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
        let request = http::Request::builder()
            .uri(format!("http://{address}/"))
            .body(Full::new(Bytes::new()))
            .unwrap();
        let upstream = tokio::time::timeout(Duration::from_secs(2), client.request(request))
            .await
            .unwrap()
            .unwrap();
        let mut response = forward_response(upstream, None);
        for name in [
            "connection",
            "x-private",
            "proxy-authenticate",
            "transfer-encoding",
        ] {
            assert!(!response.headers().contains_key(name));
        }
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        let first = tokio::time::timeout(Duration::from_secs(2), response.body_mut().frame())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(first.into_data().unwrap(), "data: first\n\n");
        release.send(()).unwrap();
        let rest = tokio::time::timeout(Duration::from_secs(2), response.into_body().collect())
            .await
            .unwrap()
            .unwrap()
            .to_bytes();
        assert_eq!(rest, "data: second\n\n");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn graceful_shutdown_allows_in_flight_responses_to_finish() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let router = Router::new().route(
            "/",
            any({
                let entered = entered.clone();
                let release = release.clone();
                move || {
                    let entered = entered.clone();
                    let release = release.clone();
                    async move {
                        entered.notify_one();
                        release.notified().await;
                        "completed response"
                    }
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(serve_until_shutdown(listener, router, async {
            let _ = stopped.await;
        }));
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let request = tokio::spawn(async move {
            client
                .get(format!("http://{address}/"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap()
        });
        tokio::time::timeout(Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        stop.send(()).unwrap();
        tokio::task::yield_now().await;
        assert!(!server.is_finished());
        release.notify_one();
        assert_eq!(request.await.unwrap(), "completed response");
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn upstream_deadline_bounds_headers_without_buffering_the_body() {
        use tokio::io::AsyncWriteExt;
        for send_headers in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                read_request_headers(&mut socket).await;
                if send_headers {
                    socket
                        .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                        .await
                        .unwrap();
                }
                core::future::pending::<()>().await;
            });
            let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
            let request = http::Request::builder()
                .uri(format!("http://{address}/"))
                .body(Full::new(Bytes::new()))
                .unwrap();
            let result = tokio::time::timeout(
                Duration::from_secs(2),
                wait_for_upstream(Duration::from_millis(100), client.request(request)),
            )
            .await;
            server.abort();
            let _ = server.await;
            let result = result.expect("header wait must finish");
            if send_headers {
                assert_eq!(result.unwrap().status(), StatusCode::OK);
            } else {
                assert_eq!(result.unwrap_err(), StatusCode::GATEWAY_TIMEOUT);
            }
        }
    }

    #[tokio::test]
    async fn request_body_limits_apply_to_buffered_and_streamed_bodies() {
        assert_eq!(
            read_request_body(Body::from("1234"), 4).await.unwrap(),
            "1234"
        );
        assert_eq!(
            read_request_body(Body::from("12345"), 4).await.unwrap_err(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        let chunks = [
            Ok::<_, std::io::Error>(Bytes::from_static(b"12")),
            Ok(Bytes::from_static(b"345")),
        ];
        let body = Body::from_stream(futures_lite::stream::iter(chunks));
        assert_eq!(
            read_request_body(body, 4).await.unwrap_err(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert!(
            read_request_body(Body::empty(), 0)
                .await
                .unwrap()
                .is_empty()
        );
        let body = Body::from_stream(futures_lite::stream::iter([Err::<Bytes, _>(
            std::io::Error::other("interrupted upload"),
        )]));
        assert_eq!(
            read_request_body(body, 4).await.unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn authorization_is_validated_and_marked_sensitive() {
        let header = authorization_header("test-key").unwrap();
        assert_eq!(header, "Bearer test-key");
        assert!(header.is_sensitive());
        assert!(!format!("{header:?}").contains("test-key"));
        for key in ["", "   ", "secret\nvalue", "secret\rvalue"] {
            assert!(matches!(
                authorization_header(key),
                Err(Error::MissingApiKey | Error::InvalidApiKey)
            ));
        }
    }

    async fn read_request_headers(socket: &mut tokio::net::TcpStream) -> String {
        use tokio::io::AsyncReadExt;
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            headers.push(socket.read_u8().await.unwrap());
            assert!(headers.len() <= 8192);
        }
        String::from_utf8(headers).unwrap()
    }
}
