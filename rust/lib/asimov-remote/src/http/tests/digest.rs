// This is free and unencumbered software released into the public domain.

use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use core::sync::atomic::{AtomicUsize, Ordering};
use serde::ser::{Error as _, SerializeStruct as _};
use sha2::{Digest as _, Sha256};

const JSON: &[u8] = br#"{"hello": "world"}"#;
const JSON_SHA256: &str = "sha-256=:X48E9qOokqqrvdts8nOJRJN3OWDUoyWxBf7kbu9DBPE=:";

#[test]
fn rfc9530_vectors_and_binary_content_use_structured_byte_sequences() {
    // RFC 9530 Appendix D, B.1/B.2, plus binary data. Expected values are
    // independent fixtures, not produced by the implementation under test.
    for (body, expected) in [
        (JSON, JSON_SHA256),
        (
            b"".as_slice(),
            "sha-256=:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=:",
        ),
        (
            b"{\"hello\": \"world\"}\n".as_slice(),
            "sha-256=:RK/0qy18MlBSVnWgjwz6lZEWjP/lF5HF9bvEF8FabDg=:",
        ),
        (
            b"\0\xff\r\n".as_slice(),
            "sha-256=:6UifN/swUenvodyRYATXJ057Y5deMglwiUcmfyOTqb4=:",
        ),
    ] {
        let options = DigestOptions {
            representation: true,
            ..DigestOptions::default()
        };
        let headers = options.headers(body);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers["content-digest"], expected);
        assert_eq!(headers["repr-digest"], expected);
    }
}

#[test]
fn digest_fields_are_independent_and_disabled_fields_are_omitted() {
    for content in [false, true] {
        for representation in [false, true] {
            let options = DigestOptions {
                content,
                representation,
                ..DigestOptions::default()
            };
            let headers = options.headers(JSON);
            for (name, enabled) in [("content-digest", content), ("repr-digest", representation)] {
                if enabled {
                    assert_eq!(headers[name], JSON_SHA256);
                } else {
                    assert!(!headers.contains_key(name));
                }
            }
            assert_eq!(
                headers.len(),
                usize::from(content) + usize::from(representation)
            );
        }
    }
}

#[test]
fn preferences_are_integer_weights_in_the_rfc_range() {
    for value in 0..=u8::MAX {
        let preference = DigestPreference::new(value);
        assert_eq!(preference.is_some(), value <= 10);
        if let Some(preference) = preference {
            let options = DigestOptions {
                content: false,
                want_content: Some(preference),
                want_representation: Some(preference),
                ..DigestOptions::default()
            };
            let headers = options.headers(JSON);
            assert_eq!(headers.len(), 2);
            assert_eq!(headers["want-content-digest"], format!("sha-256={value}"));
            assert_eq!(headers["want-repr-digest"], format!("sha-256={value}"));
        }
    }
}

// Server-side checks hash the received bytes rather than reusing the client's
// header builder. Fixed RFC vectors above separately check the field encoding.
pub(super) fn sha256_field(body: &[u8]) -> String {
    format!("sha-256=:{}:", STANDARD.encode(Sha256::digest(body)))
}

fn header_values<'a>(headers: &'a str, name: &str) -> Vec<&'a str> {
    headers
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name).then_some(value.trim())
        })
        .collect()
}

pub(super) fn assert_default_digest(headers: &str, body: &[u8]) {
    assert_eq!(
        header_values(headers, "content-digest"),
        [sha256_field(body)]
    );
    for name in [
        "repr-digest",
        "want-content-digest",
        "want-repr-digest",
        "digest",
        "want-digest",
        "signature",
        "signature-input",
    ] {
        assert!(header_values(headers, name).is_empty(), "unexpected {name}");
    }
}

#[tokio::test]
async fn all_fields_cover_the_once_serialized_body_on_the_wire() {
    struct CountedJson(AtomicUsize);

    impl Serialize for CountedJson {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let count = self.0.fetch_add(1, Ordering::SeqCst);
            let mut object = serializer.serialize_struct("CountedJson", 3)?;
            object.serialize_field("z", "Українське \"\\\r\n\0")?;
            object.serialize_field("a", &1.0)?;
            object.serialize_field("count", &count)?;
            object.end()
        }
    }

    let (url, received, resume, server) = http1_server(200, b"", b"").await;
    let executor = Executor::new(url, Some("token"))
        .unwrap()
        .with_digests(DigestOptions {
            representation: true,
            want_content: DigestPreference::new(10),
            want_representation: DigestPreference::new(0),
            ..DigestOptions::default()
        });
    let request = CountedJson(AtomicUsize::new(0));
    let mut stream = timeout(DEADLINE, executor.clone().post_jsonl("custom", &request))
        .await
        .unwrap()
        .unwrap();
    let received = timeout(DEADLINE, received).await.unwrap().unwrap();
    let (headers, body) = received.split_once("\r\n\r\n").unwrap();
    assert_eq!(request.0.load(Ordering::SeqCst), 1);
    // This ordering and float spelling differ from JCS, even with `jcs` enabled.
    assert_eq!(
        body,
        r#"{"z":"Українське \"\\\r\n\u0000","a":1.0,"count":0}"#
    );
    let expected = sha256_field(body.as_bytes());
    assert_eq!(
        header_values(headers, "content-digest"),
        [expected.as_str()]
    );
    assert_eq!(header_values(headers, "repr-digest"), [expected.as_str()]);
    assert_eq!(
        header_values(headers, "want-content-digest"),
        ["sha-256=10"]
    );
    assert_eq!(header_values(headers, "want-repr-digest"), ["sha-256=0"]);
    assert_eq!(
        header_values(headers, "content-length"),
        [format!("{}", body.len())]
    );
    assert!(header_values(headers, "content-encoding").is_empty());
    resume.send(()).unwrap();
    assert!(timeout(DEADLINE, stream.next()).await.unwrap().is_none());
    server.await.unwrap();
}

#[test]
fn prepared_request_exposes_the_final_signing_components() {
    let executor = Executor::new("https://EXAMPLE.com:443/api", Some("token")).unwrap();
    let request = executor
        .prepare_json_request("custom%2Fpath?x=1&x=2#ignored", &json!({}))
        .unwrap();
    assert_eq!(request.method(), reqwest::Method::POST);
    assert_eq!(
        request.url().as_str(),
        "https://example.com/api/custom%2Fpath?x=1&x=2"
    );
    let headers = request.headers();
    assert_eq!(headers["authorization"], "Bearer token");
    assert!(headers["authorization"].is_sensitive());
    assert_eq!(headers["accept"], "application/jsonl");
    assert_eq!(headers["content-type"], "application/json");
    assert_eq!(headers["content-length"], "2");
    assert_eq!(headers["content-digest"], sha256_field(b"{}"));
    assert_eq!(request.body().unwrap().as_bytes().unwrap(), b"{}");
}

#[tokio::test]
async fn serialization_and_origin_errors_prevent_dispatch() {
    struct InvalidJson;

    impl Serialize for InvalidJson {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(S::Error::custom("serialization failed"))
        }
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let executor = Executor::new(
        format!("http://{}", listener.local_addr().unwrap()),
        Some("token"),
    )
    .unwrap();
    assert!(matches!(executor.post_jsonl("fetch", &InvalidJson).await,
        Err(Error::Http(error)) if error.is_builder()));
    assert!(matches!(
        executor
            .post_jsonl("https://elsewhere.example/fetch", &InvalidJson)
            .await,
        Err(Error::InvalidBaseUrl(_))
    ));
    assert!(
        timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn redirects_cannot_retarget_or_rewrite_a_prepared_request() {
    for status in [301, 302, 303, 307, 308] {
        for location in ["/redirected", "http://127.0.0.1:1/redirected", ""] {
            let headers = if location.is_empty() {
                String::new()
            } else {
                format!("Location: {location}\r\n")
            };
            let (url, request, resume, server) =
                http1_server_with_headers(status, &headers, b"", b"").await;
            let executor = Executor::new(url, Some("token")).unwrap();
            assert!(
                matches!(timeout(DEADLINE, executor.post_jsonl("fetch", &json!({}))).await.unwrap(),
                Err(Error::Redirect(code)) if code.as_u16() == status)
            );
            let request = timeout(DEADLINE, request).await.unwrap().unwrap();
            let (headers, body) = request.split_once("\r\n\r\n").unwrap();
            assert_default_digest(headers, body.as_bytes());
            drop(resume);
            server.await.unwrap();
        }
    }
}
