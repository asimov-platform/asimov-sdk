// This is free and unencumbered software released into the public domain.

#![cfg(feature = "jcs")]

extern crate alloc;

use alloc::{format, string::ToString, vec};
use asimov_remote::{FetcherRequest, ListerOptions, ListerRequest};
use core::fmt;
use serde_json::json;

fn assert_fetcher_encoding(request: &FetcherRequest, expected: &str) {
    assert_eq!(request.to_canonical_json().unwrap(), expected);
    assert_eq!(
        request.to_canonical_json_bytes().unwrap(),
        expected.as_bytes(),
    );
}

fn assert_lister_encoding<T: Clone + fmt::Display, F: fmt::Display>(
    request: &ListerRequest<T, F>,
    expected: &str,
) {
    assert_eq!(request.to_canonical_json().unwrap(), expected);
    assert_eq!(
        request.to_canonical_json_bytes().unwrap(),
        expected.as_bytes(),
    );
}

#[test]
fn fetcher_sorts_all_option_groups_and_preserves_array_order() {
    let request: FetcherRequest = serde_json::from_value(json!({
        "urls": ["https://example.com/z", "https://example.com/a"],
        "options": {
            "output": "jsonl",
            "other": ["--z", "--a", ""],
            "maxAge": "90s 123ms 456us 789ns",
            "jev": "The name is \"Українське\"",
            "jq": ".",
            "deadline": "1ns"
        }
    }))
    .unwrap();
    assert_fetcher_encoding(
        &request,
        concat!(
            r#"{"options":{"deadline":"1ns","jev":"The name is \"Українське\"","jq":".","#,
            r#""max-age":"1m 30s 123ms 456us 789ns","other":["--z","--a",""],"output":"jsonl"},"#,
            r#""urls":["https://example.com/z","https://example.com/a"]}"#,
        ),
    );
}

#[test]
fn lister_sorts_all_option_groups_and_preserves_array_order() {
    let request: ListerRequest = serde_json::from_value(json!({
        "url": "https://example.com/collection",
        "options": {
            "sort": "-name,first-name",
            "before": "urn:entry:9",
            "after": "urn:entry:1",
            "offset": 20,
            "limit": 10,
            "output": "csv",
            "other": ["--z", "--a", ""],
            "max_age": "1ns",
            "jev": "name",
            "jq": ".",
            "deadline": "1s"
        }
    }))
    .unwrap();
    assert_lister_encoding(
        &request,
        concat!(
            r#"{"options":{"after":"urn:entry:1","before":"urn:entry:9","deadline":"1s","#,
            r#""jev":"name","jq":".","limit":10,"max-age":"1ns","offset":20,"#,
            r#""other":["--z","--a",""],"output":"csv","sort":"-name,first-name"},"#,
            r#""url":"https://example.com/collection"}"#,
        ),
    );
}

#[test]
fn missing_and_empty_options_are_omitted() {
    for options in [None, Some(json!({})), Some(json!({"other": []}))] {
        let mut fetch = json!({"urls": ["example:"]});
        let mut list = json!({"url": "example:"});
        if let Some(options) = options {
            fetch["options"] = options.clone();
            list["options"] = options;
        }
        let fetch: FetcherRequest = serde_json::from_value(fetch).unwrap();
        let list: ListerRequest = serde_json::from_value(list).unwrap();
        assert_fetcher_encoding(&fetch, r#"{"urls":["example:"]}"#);
        assert_lister_encoding(&list, r#"{"url":"example:"}"#);
    }
    assert_fetcher_encoding(&FetcherRequest::default(), r#"{"urls":[]}"#);
}

#[test]
fn explicit_zero_and_empty_options_are_preserved() {
    let fetch: FetcherRequest = serde_json::from_value(json!({
        "urls": ["example:"],
        "options": {"max-age": "0s", "deadline": "0s", "jev": "", "jq": ""}
    }))
    .unwrap();
    assert_fetcher_encoding(
        &fetch,
        concat!(
            r#"{"options":{"deadline":"0s","jev":"","jq":"","max-age":"0s"},"#,
            r#""urls":["example:"]}"#,
        ),
    );
    let list: ListerRequest = serde_json::from_value(json!({
        "url": "example:",
        "options": {
            "sort": "", "before": "", "after": "", "offset": 0, "limit": 0,
            "max-age": "0s", "deadline": "0s", "jev": "", "jq": ""
        }
    }))
    .unwrap();
    assert_lister_encoding(
        &list,
        concat!(
            r#"{"options":{"after":"","before":"","deadline":"0s","jev":"","jq":"","#,
            r#""limit":0,"max-age":"0s","offset":0,"sort":""},"url":"example:"}"#,
        ),
    );
}

#[test]
fn strings_use_jcs_escaping_without_unicode_normalization() {
    let value = "\u{0}\u{1f}\u{8}\t\n\u{c}\r\"\\/é😀\u{2028}\u{2029}e\u{301}";
    let fetch = FetcherRequest::new(value, Default::default());
    let list: ListerRequest = ListerRequest::new(value, Default::default());
    assert_fetcher_encoding(
        &fetch,
        concat!(
            r#"{"urls":["\u0000\u001f\b\t\n\f\r\"\\/é😀"#,
            "\u{2028}\u{2029}e\u{301}",
            r#""]}"#,
        ),
    );
    assert_lister_encoding(
        &list,
        concat!(
            r#"{"url":"\u0000\u001f\b\t\n\f\r\"\\/é😀"#,
            "\u{2028}\u{2029}e\u{301}",
            r#""}"#,
        ),
    );
}

#[test]
fn typed_keys_and_formats_only_need_display() {
    #[derive(Clone)]
    struct Property;

    impl fmt::Display for Property {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("name")
        }
    }

    struct Format;

    impl fmt::Display for Format {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("csv")
        }
    }

    let request = ListerRequest::new(
        "example:",
        ListerOptions::<Property, Format>::builder()
            .sort(vec![(Property, true).into()].into())
            .output(asimov_patterns::OutputFormat::Other(Format))
            .build(),
    );
    assert_lister_encoding(
        &request,
        r#"{"options":{"output":"csv","sort":"-name"},"url":"example:"}"#,
    );
}

#[test]
fn safe_integer_counts_are_exact() {
    for count in [
        0,
        1,
        u32::MAX as u64,
        9_007_199_254_740_990,
        9_007_199_254_740_991,
    ] {
        let Ok(count) = usize::try_from(count) else {
            continue;
        };
        let request: ListerRequest = ListerRequest::new(
            "example:",
            ListerOptions::builder().offset(count).limit(count).build(),
        );
        assert_lister_encoding(
            &request,
            &format!(r#"{{"options":{{"limit":{count},"offset":{count}}},"url":"example:"}}"#),
        );
    }
}

#[test]
fn counts_above_the_safe_integer_range_are_rejected() {
    for count in [9_007_199_254_740_992, 9_007_199_254_740_993, u64::MAX] {
        let Ok(count) = usize::try_from(count) else {
            continue;
        };
        for name in ["offset", "limit"] {
            let mut request: ListerRequest = ListerRequest::new("example:", Default::default());
            match name {
                "offset" => request.options.offset = Some(count),
                "limit" => request.options.limit = Some(count),
                _ => unreachable!(),
            }
            for error in [
                request.to_canonical_json().unwrap_err(),
                request.to_canonical_json_bytes().unwrap_err(),
            ] {
                assert!(error.is_data());
                assert!(error.to_string().contains(name));
            }
            // The stricter range belongs to the canonicalization methods.
            let json = serde_json::to_value(&request).unwrap();
            assert_eq!(json["options"][name].as_u64(), Some(count as u64));
        }
    }
}
