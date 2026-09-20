// This is free and unencumbered software released into the public domain.

#![cfg(feature = "reqwest")]

use crate::{FetchRequest, ListRequest};
use alloc::{format, string::String, vec::Vec};
use core::fmt;
use futures::{Stream, StreamExt, stream::BoxStream};
use reqwest::{Client, Url, header::ACCEPT};
use serde::Serialize;
use serde_json::Value;

/// An asynchronous HTTP client for <https://asimov.social>.
#[derive(Clone)]
pub struct SocialClient {
    client: Client,
    api_token: String,
    base_url: Url,
}

impl fmt::Debug for SocialClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SocialClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

/// A stream of JSONL records that terminates on a body or JSON decoding error.
pub type SocialResponseStream = BoxStream<'static, Result<Value, SocialClientError>>;

/// An error sending a social request or decoding its JSONL response.
#[derive(Debug, thiserror::Error)]
pub enum SocialClientError {
    #[error(transparent)]
    Url(#[from] url::ParseError),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl SocialClient {
    /// Creates a rustls-backed client for <https://asimov.social>.
    ///
    /// The API token is sent as bearer authentication on every request.
    pub fn new(api_token: impl Into<String>) -> Result<Self, SocialClientError> {
        Self::with_base_url(api_token, "https://asimov.social")
    }

    /// Creates a client with a custom base URL, including an optional path prefix.
    ///
    /// A trailing slash is optional. Query strings and fragments are ignored.
    pub fn with_base_url(
        api_token: impl Into<String>,
        base_url: impl AsRef<str>,
    ) -> Result<Self, SocialClientError> {
        let mut base_url = Url::parse(base_url.as_ref())?;
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }
        base_url.set_query(None);
        base_url.set_fragment(None);

        Ok(Self {
            client: Client::builder().use_rustls_tls().build()?,
            api_token: api_token.into(),
            base_url,
        })
    }

    /// POSTs a fetch request to `fetch` under the configured base URL.
    ///
    /// Request and HTTP status errors are returned immediately. The stream yields
    /// JSONL records as they arrive and terminates on a body or JSON decoding error.
    pub async fn fetch(
        &self,
        request: FetchRequest,
    ) -> Result<SocialResponseStream, SocialClientError> {
        self.post("fetch", &request).await
    }

    /// POSTs a list request to `list` under the configured base URL.
    ///
    /// Request and HTTP status errors are returned immediately. The stream yields
    /// JSONL records as they arrive and terminates on a body or JSON decoding error.
    pub async fn list(
        &self,
        request: ListRequest,
    ) -> Result<SocialResponseStream, SocialClientError> {
        self.post("list", &request).await
    }

    async fn post(
        &self,
        endpoint: &str,
        request: &impl Serialize,
    ) -> Result<SocialResponseStream, SocialClientError> {
        let response = self
            .client
            .post(self.base_url.join(endpoint)?)
            .bearer_auth(&self.api_token)
            .header(ACCEPT, "application/jsonl")
            .json(request)
            .send()
            .await?
            .error_for_status()?;

        Ok(decode_jsonl(response.bytes_stream()).boxed())
    }
}

/// Buffers only the current record, preserving UTF-8 across arbitrary chunks.
fn decode_jsonl<B: AsRef<[u8]> + Send + 'static>(
    chunks: impl Stream<Item = Result<B, reqwest::Error>> + Send + 'static,
) -> impl Stream<Item = Result<Value, SocialClientError>> + Send + 'static {
    async_stream::try_stream! {
        futures::pin_mut!(chunks);
        let mut line = Vec::new();

        while let Some(chunk) = chunks.next().await {
            let chunk = chunk?;
            for part in chunk.as_ref().split_inclusive(|byte| *byte == b'\n') {
                line.extend_from_slice(part);
                if part.last() == Some(&b'\n') {
                    if !line.iter().all(u8::is_ascii_whitespace) {
                        let value = serde_json::from_slice(&line)?;
                        line.clear();
                        yield value;
                    } else {
                        line.clear();
                    }
                }
            }
        }

        if !line.iter().all(u8::is_ascii_whitespace) {
            yield serde_json::from_slice(&line)?;
        }
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use alloc::{string::ToString, vec};
    use futures::{TryStreamExt, stream};
    use serde_json::json;
    use std::time::Duration;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
        time::timeout,
    };

    #[tokio::test]
    async fn decodes_arbitrary_chunks_and_unterminated_final_record() {
        let body = "\r\n{\"name\":\"café 🦀\"}\r\n \t\n[1,true]\nnull";
        for size in 1..=body.len() {
            let chunks: Vec<_> = body
                .as_bytes()
                .chunks(size)
                .map(|b| Ok(b.to_vec()))
                .collect();
            let values: Vec<_> = decode_jsonl(stream::iter(chunks))
                .try_collect()
                .await
                .unwrap();
            assert_eq!(
                values,
                vec![json!({"name": "café 🦀"}), json!([1, true]), Value::Null]
            );
        }
    }

    #[tokio::test]
    async fn empty_and_blank_responses_produce_no_records() {
        for body in ["", " \r\n\t\n "] {
            let values: Vec<_> = decode_jsonl(stream::iter([Ok(body.as_bytes())]))
                .try_collect()
                .await
                .unwrap();
            assert!(values.is_empty());
        }
    }

    #[tokio::test]
    async fn decoding_errors_terminate_the_stream() {
        for invalid in [b"invalid\n".as_slice(), b"{", b"\"\xff\"\n"] {
            let records = decode_jsonl(stream::iter([
                Ok(b"1\n".as_slice()),
                Ok(invalid),
                Ok(b"\n2\n".as_slice()),
            ]));
            futures::pin_mut!(records);
            assert_eq!(records.next().await.unwrap().unwrap(), json!(1));
            assert!(matches!(
                records.next().await,
                Some(Err(SocialClientError::Json(_)))
            ));
            assert!(records.next().await.is_none());
        }
    }

    #[tokio::test]
    async fn body_errors_terminate_the_stream() {
        let error = Client::new().get("://invalid").build().unwrap_err();
        let records = decode_jsonl(stream::iter([
            Ok(b"1\n".as_slice()),
            Err(error),
            Ok(b"2\n".as_slice()),
        ]));
        futures::pin_mut!(records);
        assert_eq!(records.next().await.unwrap().unwrap(), json!(1));
        assert!(matches!(
            records.next().await,
            Some(Err(SocialClientError::Http(_)))
        ));
        assert!(records.next().await.is_none());
    }

    #[test]
    fn configures_default_url_and_redacts_token() {
        let client = SocialClient::new("secret-token").unwrap();
        assert_eq!(client.base_url.as_str(), "https://asimov.social/");
        assert!(!format!("{client:?}").contains("secret-token"));
        assert!(matches!(
            SocialClient::with_base_url("token", "not a URL"),
            Err(SocialClientError::Url(_))
        ));
    }

    #[tokio::test]
    async fn posts_authenticated_requests_and_streams_before_eof() {
        for (endpoint, prefix) in [("fetch", "/api"), ("list", "/api/")] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base_url = format!("http://{}{prefix}", listener.local_addr().unwrap());
            let (request_tx, request_rx) = oneshot::channel();
            let (continue_tx, continue_rx) = oneshot::channel();
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
                socket.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/jsonl\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n1\n\r\n",
                ).await.unwrap();

                // The response cannot finish until the client consumes its first record.
                continue_rx.await.unwrap();
                socket.write_all(b"1\r\n2\r\n0\r\n\r\n").await.unwrap();
            });

            let client = SocialClient::with_base_url("test-token", base_url).unwrap();
            let url = "https://example.com/profile".to_string();
            let (mut records, expected_body) = if endpoint == "fetch" {
                let request = FetchRequest {
                    urls: vec![url.clone()],
                    ..Default::default()
                };
                (
                    timeout(Duration::from_secs(5), client.fetch(request))
                        .await
                        .unwrap()
                        .unwrap(),
                    json!({"urls": [url]}),
                )
            } else {
                let request = ListRequest {
                    url: url.clone(),
                    ..Default::default()
                };
                (
                    timeout(Duration::from_secs(5), client.list(request))
                        .await
                        .unwrap()
                        .unwrap(),
                    json!({"url": url}),
                )
            };
            drop(client);

            let request = request_rx.await.unwrap();
            let (headers, body) = request.split_once("\r\n\r\n").unwrap();
            let headers = headers.to_ascii_lowercase();
            assert!(headers.starts_with(&format!("post /api/{endpoint} http/1.1\r\n")));
            assert!(headers.contains("\r\nauthorization: bearer test-token"));
            assert!(headers.contains("\r\naccept: application/jsonl"));
            assert!(headers.contains("\r\ncontent-type: application/json"));
            assert_eq!(serde_json::from_str::<Value>(body).unwrap(), expected_body);

            assert_eq!(
                timeout(Duration::from_secs(5), records.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap(),
                json!(1),
            );
            continue_tx.send(()).unwrap();
            assert_eq!(records.next().await.unwrap().unwrap(), json!(2));
            assert!(records.next().await.is_none());
            server.await.unwrap();
        }
    }
}
