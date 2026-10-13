// This is free and unencumbered software released into the public domain.

#![cfg(feature = "serde")]

extern crate alloc;

use alloc::{format, string::String, vec};
use asimov_remote::{FetcherRequest, ListerOptions, ListerRequest};
use core::{fmt, str::FromStr, time::Duration};
use serde_json::json;

#[test]
fn fetcher_body_round_trips_all_option_groups_and_multiple_urls() {
    let wire = json!({
        "urls": ["https://example.com/alice", "https://example.com/bob"],
        "options": {
            "output": "jsonl",
            "other": ["--custom", "literal argument", ""],
            "max-age": "1m 30s 123ms 456us 789ns",
            "jev": "The name is \"Українське\"",
            "jq": "select(.name == \"Ada Lovelace\")",
            "deadline": "1ns"
        }
    });
    let request: FetcherRequest = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(request.urls.len(), 2);
    assert_eq!(request.options.output.as_deref(), Some("jsonl"));
    assert_eq!(request.options.other, ["--custom", "literal argument", ""]);
    assert_eq!(
        request.caching.max_age,
        Some(Duration::new(90, 123_456_789))
    );
    assert_eq!(
        request.filtering.jev.as_deref(),
        Some("The name is \"Українське\"")
    );
    assert_eq!(request.timing.deadline, Some(Duration::from_nanos(1)));
    assert_eq!(serde_json::to_value(&request).unwrap(), wire);
    let bytes = serde_json::to_vec(&request).unwrap();
    assert_eq!(
        serde_json::from_slice::<FetcherRequest>(&bytes).unwrap(),
        request
    );
}

#[test]
fn lister_body_round_trips_all_option_groups() {
    let wire = json!({
        "url": "https://example.com/collection",
        "options": {
            "sort": "-name,first-name",
            "before": "urn:entry:9",
            "after": "urn:entry:1",
            "offset": 0,
            "limit": 0,
            "output": "csv",
            "other": ["--custom", "literal argument", ""],
            "max-age": "1ns",
            "jev": "",
            "jq": "",
            "deadline": "0s"
        }
    });
    let request: ListerRequest = serde_json::from_value(wire.clone()).unwrap();
    let keys = request.options.sort.as_ref().unwrap().keys();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].key(), "name");
    assert!(keys[0].descending());
    assert_eq!(keys[1].key(), "first-name");
    assert!(!keys[1].descending());
    assert_eq!(request.options.offset, Some(0));
    assert_eq!(request.options.limit, Some(0));
    assert_eq!(request.options.output.as_ref().unwrap().as_str(), "csv");
    assert_eq!(request.caching.max_age, Some(Duration::from_nanos(1)));
    assert_eq!(request.filtering.jev.as_deref(), Some(""));
    assert_eq!(request.filtering.jq.as_deref(), Some(""));
    assert_eq!(request.timing.deadline, Some(Duration::ZERO));
    assert_eq!(serde_json::to_value(&request).unwrap(), wire);
    let bytes = serde_json::to_vec(&request).unwrap();
    assert_eq!(
        serde_json::from_slice::<ListerRequest>(&bytes).unwrap(),
        request
    );
}

#[test]
fn missing_or_empty_options_default_and_serialize_as_omitted() {
    for options in [None, Some(json!({})), Some(json!({"other": []}))] {
        let mut fetch = json!({"urls": ["example:"]});
        let mut list = json!({"url": "example:"});
        if let Some(options) = options {
            fetch["options"] = options.clone();
            list["options"] = options;
        }
        let fetch: FetcherRequest = serde_json::from_value(fetch).unwrap();
        let list: ListerRequest = serde_json::from_value(list).unwrap();
        assert_eq!(fetch, FetcherRequest::new("example:", Default::default()));
        assert_eq!(list, ListerRequest::new("example:", Default::default()));
        assert_eq!(
            serde_json::to_value(fetch).unwrap(),
            json!({"urls": ["example:"]})
        );
        assert_eq!(
            serde_json::to_value(list).unwrap(),
            json!({"url": "example:"})
        );
    }
}

#[test]
fn explicit_empty_sort_and_shared_options_are_preserved() {
    let wire = json!({
        "url": "example:",
        "options": {"sort": "", "max-age": "0s", "deadline": "0s", "jev": "", "jq": ""}
    });
    let request: ListerRequest = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(request).unwrap(), wire);

    let wire = json!({
        "urls": ["example:"],
        "options": {"max-age": "0s", "deadline": "0s", "jev": "", "jq": ""}
    });
    let request: FetcherRequest = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(request).unwrap(), wire);
}

#[test]
fn caching_aliases_are_canonicalized_and_duplicates_rejected() {
    for alias in ["max_age", "maxAge"] {
        let fetch: FetcherRequest = serde_json::from_value(json!({
            "urls": ["example:"], "options": {(alias): "1h"}
        }))
        .unwrap();
        let list: ListerRequest = serde_json::from_value(json!({
            "url": "example:", "options": {(alias): "1h"}
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(fetch).unwrap()["options"],
            json!({"max-age": "1h"})
        );
        assert_eq!(
            serde_json::to_value(list).unwrap()["options"],
            json!({"max-age": "1h"})
        );
        let options = format!(r#"{{"max-age":"1h","{alias}":"1h"}}"#);
        let fetch = format!(r#"{{"urls":["example:"],"options":{options}}}"#);
        let list = format!(r#"{{"url":"example:","options":{options}}}"#);
        assert!(serde_json::from_str::<FetcherRequest>(&fetch).is_err());
        assert!(serde_json::from_str::<ListerRequest>(&list).is_err());
    }
}

#[test]
fn malformed_bodies_and_option_values_are_rejected() {
    for wire in [
        json!({}),
        json!({"urls": "example:"}),
        json!({"urls": [42]}),
    ] {
        assert!(serde_json::from_value::<FetcherRequest>(wire).is_err());
    }
    for wire in [json!({}), json!({"url": ["example:"]}), json!({"url": 42})] {
        assert!(serde_json::from_value::<ListerRequest>(wire).is_err());
    }
    for options in [
        json!(null),
        json!({"deadline": "yesterday"}),
        json!({"max-age": 1}),
        json!({"jev": false}),
        json!({"jq": []}),
        json!({"output": {"Other": "jsonl"}}),
        json!({"other": "--flag"}),
    ] {
        assert!(
            serde_json::from_value::<FetcherRequest>(json!({
                "urls": ["example:"], "options": options
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<ListerRequest>(json!({
                "url": "example:", "options": options
            }))
            .is_err()
        );
    }
    for options in [
        json!({"sort": []}),
        json!({"offset": -1}),
        json!({"limit": "10"}),
    ] {
        assert!(
            serde_json::from_value::<ListerRequest>(json!({
                "url": "example:", "options": options
            }))
            .is_err()
        );
    }
}

// Serialization requires Display, without requiring FromStr or Serde.
#[derive(Clone)]
struct Property;

impl fmt::Display for Property {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("name")
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Format;

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("csv")
    }
}

impl FromStr for Format {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        (value == "csv").then_some(Self).ok_or("unsupported format")
    }
}

#[test]
fn typed_lister_serialization_and_custom_format_parsing_need_no_serde_bounds() {
    let request = ListerRequest::new(
        "example:",
        ListerOptions::<Property, Format>::builder()
            .sort(vec![(Property, true).into()].into())
            .output(asimov_patterns::OutputFormat::Other(Format))
            .build(),
    );
    let wire = serde_json::to_string(&request).unwrap();
    let parsed: ListerRequest<String, Format> = serde_json::from_str(&wire).unwrap();
    assert_eq!(
        parsed.options.output,
        Some(asimov_patterns::OutputFormat::Other(Format))
    );
    assert_eq!(
        serde_json::to_value(parsed).unwrap(),
        json!({
            "url": "example:", "options": {"sort": "-name", "output": "csv"}
        })
    );
    assert!(
        serde_json::from_str::<ListerRequest<String, Format>>(
            r#"{"url":"example:","options":{"output":"invalid"}}"#
        )
        .is_err()
    );
}
