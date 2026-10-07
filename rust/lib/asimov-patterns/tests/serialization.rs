// This is free and unencumbered software released into the public domain.

#![cfg(feature = "serde")]

extern crate alloc;

use alloc::format;
use asimov_patterns::{
    CachingOptions, FetcherOptions, FilteringOptions, ListerOptions, TimingOptions,
};
use core::time::Duration;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
struct Options {
    #[serde(flatten)]
    lister: ListerOptions,
    #[serde(flatten)]
    caching: CachingOptions,
    #[serde(flatten)]
    filtering: FilteringOptions,
    #[serde(flatten)]
    timing: TimingOptions,
}

#[test]
fn flattened_options_match_the_remote_http_protocol() {
    let wire = json!({
        "sort": "-name,date", "before": "urn:item:b", "after": "urn:item:a",
        "offset": 0, "limit": 25, "output": "jsonl", "other": ["--custom=a b"],
        "max-age": "1h", "jev": "The name is Ukrainian", "jq": "select(.name)",
        "deadline": "1m 250ms"
    });
    let options: Options = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(options.timing.deadline, Some(Duration::from_millis(60250)));
    assert_eq!(options.caching.max_age, Some(Duration::from_secs(3600)));
    assert_eq!(serde_json::to_value(&options).unwrap(), wire);
    assert_eq!(serde_json::to_value(Options::default()).unwrap(), json!({}));
    assert_eq!(
        serde_json::from_value::<Options>(json!({})).unwrap(),
        Options::default()
    );

    let fetcher: FetcherOptions = serde_json::from_value(json!({"output":"jsonl"})).unwrap();
    assert!(fetcher.other.is_empty());
    assert_eq!(
        serde_json::to_value(fetcher).unwrap(),
        json!({"output":"jsonl"})
    );
}

#[test]
fn preserves_explicit_zero_and_empty_values_but_rejects_bad_types() {
    let options: Options = serde_json::from_value(json!({
        "deadline": "0s", "max-age": "0s", "limit": 0, "jq": "", "jev": ""
    }))
    .unwrap();
    assert_eq!(options.timing.deadline, Some(Duration::ZERO));
    assert_eq!(options.caching.max_age, Some(Duration::ZERO));
    assert_eq!(options.filtering.jq.as_deref(), Some(""));
    for wire in [
        json!({"deadline": "yesterday"}),
        json!({"deadline": -1}),
        json!({"max-age": {"secs": 1}}),
        json!({"sort": ["name"]}),
        json!({"output": {"Other": "jsonl"}}),
        json!({"other": "--flag"}),
    ] {
        assert!(serde_json::from_value::<Options>(wire).is_err());
    }
}

#[test]
fn caching_aliases_parse_but_always_serialize_as_kebab_case() {
    for name in ["max-age", "max_age", "maxAge"] {
        for (value, expected) in [
            (json!("1h 250ms"), Some(Duration::from_millis(3_600_250))),
            (json!("0s"), Some(Duration::ZERO)),
            (json!(null), None),
        ] {
            let wire = json!({(name): value});
            let caching: CachingOptions = serde_json::from_value(wire.clone()).unwrap();
            let options: Options = serde_json::from_value(wire).unwrap();
            assert_eq!(caching.max_age, expected);
            assert_eq!(options.caching, caching);
            let canonical = if expected.is_some() {
                json!({"max-age": value})
            } else {
                json!({})
            };
            assert_eq!(serde_json::to_value(caching).unwrap(), canonical);
            assert_eq!(serde_json::to_value(options).unwrap(), canonical);
        }
        for value in [json!("invalid"), json!(42), json!({"secs": 1})] {
            let wire = json!({(name): value});
            assert!(serde_json::from_value::<CachingOptions>(wire.clone()).is_err());
            assert!(serde_json::from_value::<Options>(wire).is_err());
        }
    }
}

#[test]
fn duplicate_caching_names_are_rejected_even_when_equal_or_null() {
    for first in ["max-age", "max_age", "maxAge"] {
        for second in ["max-age", "max_age", "maxAge"] {
            for (a, b) in [
                (r#""1h""#, r#""1h""#),
                (r#""1h""#, r#""2h""#),
                ("null", r#""1h""#),
                (r#""1h""#, "null"),
            ] {
                let wire = format!(r#"{{"{first}":{a},"{second}":{b}}}"#);
                for error in [
                    serde_json::from_str::<CachingOptions>(&wire).unwrap_err(),
                    serde_json::from_str::<Options>(&wire).unwrap_err(),
                ] {
                    assert!(error.is_data(), "{wire}: {error}");
                    assert!(format!("{error}").contains("duplicate field `max-age`"));
                }
            }
        }
    }
}
