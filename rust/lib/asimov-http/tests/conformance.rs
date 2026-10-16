// This is free and unencumbered software released into the public domain.

#![cfg(feature = "std")]

extern crate alloc;

use alloc::{format, string::String, vec, vec::Vec};
use asimov_http::{
    Component, ContentDigest, Error, FieldSection, Message, SecretKey, SignatureInput,
    SignatureParams, Signer, StructuredFieldType, VerificationPolicy, Verifier,
    body::{sign_request, verify_request},
    key_id,
};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use http::{HeaderMap, HeaderValue, Method, Request, StatusCode};

// RFC 9421 B.1.4/B.2.6: independently specified key, base, and signature.
const INPUT: &str = r#"("date" "@method" "@path" "@authority" "content-type" "content-length");created=1618884473;keyid="test-key-ed25519""#;
const BASE: &str = concat!(
    "\"date\": Tue, 20 Apr 2021 02:07:55 GMT\n",
    "\"@method\": POST\n\"@path\": /foo\n\"@authority\": example.com\n",
    "\"content-type\": application/json\n\"content-length\": 18\n",
    "\"@signature-params\": (\"date\" \"@method\" \"@path\" \"@authority\" \"content-type\" \"content-length\");created=1618884473;keyid=\"test-key-ed25519\"",
);
const SIGNATURE: &str =
    "wqcAqbmYJ2ji2glfAMaRy4gruYYnx2nEFN2HN6jrnDnQCK1u02Gb04v9EDgwUPiu4A0w6vuQv5lIp5WPpBKRCw==";

fn key() -> SecretKey {
    let bytes = URL_SAFE_NO_PAD
        .decode("n4Ni-HpISpVObnQMW0wOhCKROaIKqKtW_2ZYb2p9KcU")
        .unwrap();
    SecretKey::from_bytes(&bytes.try_into().unwrap())
}

fn message() -> Message {
    let mut headers = HeaderMap::new();
    headers.insert(
        "date",
        HeaderValue::from_static("Tue, 20 Apr 2021 02:07:55 GMT"),
    );
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    headers.insert("content-length", HeaderValue::from_static("18"));
    Message::request(
        Method::POST,
        "http://example.com/foo?param=Value&Pet=dog",
        headers,
    )
    .unwrap()
}

fn base(component: &str, message: &Message) -> String {
    let input = SignatureInput::parse(&format!("({component})")).unwrap();
    String::from_utf8(input.signature_base(message).unwrap()).unwrap()
}

#[test]
fn rfc9421_ed25519_vector_is_byte_exact() {
    let key = key();
    assert_eq!(
        URL_SAFE_NO_PAD.encode(key.public().as_bytes()),
        "JrQLj5P_89iXES9-vFgrIy29clF9CC_oPPsw3c5D0bs"
    );
    let input = SignatureInput::parse(INPUT).unwrap();
    assert_eq!(input.signature_base(&message()).unwrap(), BASE.as_bytes());
    input
        .verify(
            &message(),
            &STANDARD.decode(SIGNATURE).unwrap(),
            &key.public(),
        )
        .unwrap();
    let fields = Signer::new(key, "sig-b26", input)
        .unwrap()
        .sign(&message())
        .unwrap();
    assert_eq!(fields.signature_input, format!("sig-b26={INPUT}"));
    assert_eq!(fields.signature, format!("sig-b26=:{SIGNATURE}:"));
}

#[test]
fn digest_parser_accepts_rfc_vectors_and_rejects_bad_types_and_lengths() {
    let body = br#"{"hello": "world"}"#;
    let field = "sha-256=:X48E9qOokqqrvdts8nOJRJN3OWDUoyWxBf7kbu9DBPE=:";
    assert_eq!(ContentDigest::new(body).to_field_value(), field);
    assert_eq!(
        ContentDigest::parse(field).unwrap(),
        ContentDigest::new(body)
    );
    assert_eq!(
        ContentDigest::parse(&format!("other=:AQ==:, {field};extension=1")).unwrap(),
        ContentDigest::new(body)
    );
    for invalid in [
        "sha-256=123",
        "sha-256=:AQ==:",
        "sha-256=(123)",
        "other=:AQ==:",
        "sha-256=:!:",
        "SHA-256=:AQ==:",
    ] {
        assert!(ContentDigest::parse(invalid).is_err(), "{invalid}");
    }
    let mut headers = HeaderMap::new();
    headers.insert("content-digest", "other=:AQ==:".parse().unwrap());
    let mut trailers = HeaderMap::new();
    trailers.insert("content-digest", field.parse().unwrap());
    ContentDigest::new(body)
        .verify(&headers, &trailers)
        .unwrap();
    headers.insert(
        "content-digest",
        ContentDigest::new(b"tampered")
            .to_field_value()
            .parse()
            .unwrap(),
    );
    assert!(
        ContentDigest::new(body)
            .verify(&headers, &trailers)
            .is_err()
    );
}

#[test]
fn received_parameter_order_extensions_and_escaping_are_authenticated() {
    let value =
        r#"("@method");keyid="key\"\\id";extension="keep me";tag="tag";created=123;alg="ed25519""#;
    let input = SignatureInput::parse(value).unwrap();
    assert_eq!(input.to_field_value(), value);
    let expected = format!("\"@method\": POST\n\"@signature-params\": {value}");
    assert_eq!(
        input.signature_base(&message()).unwrap(),
        expected.as_bytes()
    );
    let key = key();
    let signature = key.sign(expected.as_bytes());
    input
        .verify(&message(), &signature.to_bytes(), &key.public())
        .unwrap();
    let reordered = SignatureInput::parse(&value.replace(
        ";extension=\"keep me\";tag=\"tag\"",
        ";tag=\"tag\";extension=\"keep me\"",
    ))
    .unwrap();
    assert!(
        reordered
            .verify(&message(), &signature.to_bytes(), &key.public())
            .is_err()
    );
    let changed = SignatureInput::parse(&value.replace("keep me", "tampered")).unwrap();
    assert!(
        changed
            .verify(&message(), &signature.to_bytes(), &key.public())
            .is_err()
    );
    let generated = SignatureInput::new(
        vec![Component::new("@method").unwrap()],
        SignatureParams {
            created: 123,
            expires: None,
            key_id: "key\"\\id".into(),
            nonce: Some("a\"b\\c".into()),
            tag: None,
        },
    )
    .unwrap();
    let parsed = SignatureInput::parse(&generated.to_field_value()).unwrap();
    assert_eq!(parsed.parameter("keyid"), Some("key\"\\id"));
    assert_eq!(parsed.parameter("nonce"), Some("a\"b\\c"));
}

#[test]
fn duplicate_components_and_invalid_identifiers_are_rejected() {
    for input in [
        r#"("@method" "@method")"#,
        r#"("content-digest";sf;tr "content-digest";tr;sf)"#,
    ] {
        assert!(matches!(
            SignatureInput::parse(input),
            Err(Error::DuplicateComponent)
        ));
    }
    for input in [
        r#"("@unknown")"#,
        r#"("Content-Type")"#,
        r#"("@signature-params")"#,
        r#"("@status";req)"#,
        r#"("@method";sf)"#,
        r#"("x";sf;bs)"#,
        r#"("x";tr=?0)"#,
        r#"("x";key=1)"#,
        r#"("@query-param")"#,
        r#"("x");created="123""#,
        r#"("x");expires=-1"#,
        r#"("x");alg="rsa-pss-sha512""#,
        r#"("x");nonce=123"#,
        r#"("x");extension=@123"#,
    ] {
        assert!(SignatureInput::parse(input).is_err(), "{input}");
    }
    let too_many = format!(
        "({})",
        (0..65)
            .map(|i| format!("\"x-{i}\""))
            .collect::<Vec<_>>()
            .join(" ")
    );
    assert!(matches!(
        SignatureInput::parse(&too_many),
        Err(Error::Limit)
    ));
}

#[test]
fn repeated_empty_binary_and_structured_fields_are_canonicalized() {
    let mut message = message();
    message
        .headers
        .append("x-list", "  first\t".parse().unwrap());
    message
        .headers
        .append("x-list", "\tsecond ".parse().unwrap());
    message
        .headers
        .insert("x-empty", HeaderValue::from_static(""));
    message
        .headers
        .insert("x-bytes", HeaderValue::from_bytes(b" \xff\x80\t").unwrap());
    message
        .headers
        .append("content-digest", "other=:AQ==:".parse().unwrap());
    message.headers.append(
        "content-digest",
        ContentDigest::new(b"abc").to_field_value().parse().unwrap(),
    );
    assert!(base(r#""x-list""#, &message).starts_with("\"x-list\": first, second\n"));
    assert!(base(r#""x-empty""#, &message).starts_with("\"x-empty\": \n"));
    assert!(base(r#""x-bytes";bs"#, &message).starts_with("\"x-bytes\";bs: :/4A=:\n"));
    assert!(base(r#""content-digest";sf;key="sha-256""#, &message).starts_with(
        "\"content-digest\";sf;key=\"sha-256\": :ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=:\n"));
    assert!(
        SignatureInput::parse(r#"("absent")"#)
            .unwrap()
            .signature_base(&message)
            .is_err()
    );
    assert!(
        SignatureInput::parse(r#"("x-bytes")"#)
            .unwrap()
            .signature_base(&message)
            .is_err()
    );
    message
        .headers
        .insert("x-item", "  12.0; x=\"escaped\\\"\"".parse().unwrap());
    assert!(
        SignatureInput::parse(r#"("x-item";sf)"#)
            .unwrap()
            .signature_base(&message)
            .is_err()
    );
    let message =
        message.with_structured_field("x-item".parse().unwrap(), StructuredFieldType::Item);
    assert!(
        base(r#""x-item";sf"#, &message).starts_with("\"x-item\";sf: 12.0;x=\"escaped\\\"\"\n")
    );
}

#[test]
fn uri_method_query_and_request_response_context_are_explicit() {
    let mut request = Message::request(
        Method::from_bytes(b"custom").unwrap(),
        "https://EXAMPLE.com:443/a%2Fb?x=one%20two&empty&x=duplicate",
        HeaderMap::new(),
    )
    .unwrap();
    assert!(base(r#""@method""#, &request).starts_with("\"@method\": custom\n"));
    assert!(
        base(r#""@target-uri""#, &request)
            .contains("https://example.com/a%2Fb?x=one%20two&empty&x=duplicate")
    );
    assert!(
        SignatureInput::parse(r#"("@query-param";name="x")"#)
            .unwrap()
            .signature_base(&request)
            .is_err()
    );
    assert!(base(r#""@query-param";name="empty""#, &request).contains("name=\"empty\": \n"));
    assert!(
        SignatureInput::parse(r#"("@request-target")"#)
            .unwrap()
            .signature_base(&request)
            .is_err()
    );
    request
        .trailers
        .insert("x-trailer", "request".parse().unwrap());
    let mut response = Message::response(StatusCode::OK, HeaderMap::new(), Some(request)).unwrap();
    response
        .trailers
        .insert("x-trailer", "response".parse().unwrap());
    assert!(base(r#""x-trailer";tr;req"#, &response).contains(";tr;req: request\n"));
    assert!(base(r#""x-trailer";tr"#, &response).contains(";tr: response\n"));
    assert!(base(r#""@status""#, &response).starts_with("\"@status\": 200\n"));
    assert!(base(r#""@method";req"#, &response).starts_with("\"@method\";req: custom\n"));
    for uri in [
        "/relative",
        "https://example.com/#fragment",
        "https://user@example.com/",
    ] {
        assert!(Message::request(Method::GET, uri, HeaderMap::new()).is_err());
    }
    let request = Message::request(
        Method::GET,
        "https://[::1]:8443/?a=one%20two",
        HeaderMap::new(),
    )
    .unwrap();
    assert!(base(r#""@authority""#, &request).contains("[::1]:8443"));
    assert!(base(r#""@query-param";name="a""#, &request).contains("one+two\n"));
}

#[test]
fn buffered_request_policy_binds_body_key_metadata_and_freshness() {
    let key = key();
    let mut request = Request::post("https://example.com/fetch")
        .header("authorization", "Bearer token")
        .header("accept", "application/jsonl")
        .header("content-type", "application/json")
        .body(b"{}".to_vec())
        .unwrap();
    let policy = VerificationPolicy::request();
    let input = SignatureInput::new(
        policy.required_components.clone(),
        SignatureParams {
            created: 100,
            expires: Some(160),
            key_id: key_id(&key.public()),
            nonce: Some(STANDARD.encode([7; 16])),
            tag: policy.required_tag.clone(),
        },
    )
    .unwrap();
    let signer = Signer::new(key.clone(), "asimov", input).unwrap();
    let verifier = Verifier::new(key.public(), &key_id(&key.public()), "asimov", policy).unwrap();
    sign_request(&mut request, "https://example.com/fetch", &signer).unwrap();
    verify_request(&request, "https://example.com/fetch", &verifier, 100).unwrap();
    assert!(verify_request(&request, "https://example.com/fetch", &verifier, 69).is_err());
    assert!(verify_request(&request, "https://example.com/fetch", &verifier, 190).is_err());
    assert!(verify_request(&request, "https://example.com/other", &verifier, 100).is_err());
    request.body_mut().push(b' ');
    assert!(verify_request(&request, "https://example.com/fetch", &verifier, 100).is_err());
    request.body_mut().pop();
    request
        .headers_mut()
        .insert("authorization", "Bearer altered".parse().unwrap());
    assert!(verify_request(&request, "https://example.com/fetch", &verifier, 100).is_err());
}

#[test]
fn signature_envelopes_can_span_sections_without_losing_scope() {
    let key = key();
    let input = SignatureInput::parse(
        r#"("@status" "content-digest";tr);created=100;expires=160;keyid="key";alg="ed25519""#,
    )
    .unwrap();
    let mut message = Message::response(StatusCode::OK, HeaderMap::new(), None).unwrap();
    message.trailers.insert(
        "content-digest",
        ContentDigest::new(b"abc").to_field_value().parse().unwrap(),
    );
    let signer = Signer::new(key.clone(), "s", input.clone()).unwrap();
    let fields = signer.sign(&message).unwrap();
    message
        .headers
        .insert("signature-input", fields.signature_input.clone());
    message
        .trailers
        .insert("signature", fields.signature.clone());
    let mut policy = VerificationPolicy::new(input.components().to_vec());
    policy.require_nonce = false;
    let verifier = Verifier::new(key.public(), "key", "s", policy).unwrap();
    verifier.verify(&message, 100).unwrap();
    message
        .trailers
        .append("signature", "other=123".parse().unwrap());
    assert!(matches!(
        verifier.verify(&message, 100),
        Err(Error::InvalidField(_))
    ));
    message
        .trailers
        .insert("signature", fields.signature.clone());
    message.headers.insert(
        "content-digest",
        message.trailers.remove("content-digest").unwrap(),
    );
    assert!(verifier.verify(&message, 100).is_err());
    message.trailers.insert(
        "content-digest",
        message.headers.remove("content-digest").unwrap(),
    );
    message
        .trailers
        .insert("signature-input", fields.signature_input);
    assert!(matches!(
        verifier.verify(&message, 100),
        Err(Error::DuplicateLabel)
    ));
    assert!(SignatureInput::from_message(&message, "missing").is_err());
    assert_eq!(message.fields(FieldSection::Trailers).len(), 3);
}
