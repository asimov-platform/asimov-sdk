// This is free and unencumbered software released into the public domain.

extern crate alloc;

use alloc::string::ToString;
use asimov_directory::{UrlFile, UrlsFile};

#[test]
fn formats_round_trip_without_normalizing_urls() {
    let text = "HTTPS://Example.COM/%2f?q=%41#fragment\n";
    let file: UrlFile = text.parse().unwrap();
    assert_eq!(file.url(), text.trim_end_matches('\n'));
    assert_eq!(file.to_string(), text);
    let text = "HTTPS://Example.COM\nurn:isbn:9780140328721\nfile:///tmp/a%20b\n";
    let file: UrlsFile = text.parse().unwrap();
    assert_eq!(file.to_string(), text);
    assert_eq!("".parse::<UrlsFile>().unwrap(), UrlsFile::default());
}

#[test]
fn malformed_files_and_line_injection_are_rejected() {
    for text in [
        "https://example.com",
        "https://example.com\n\n",
        "https://example.com\r\n",
        "\n",
        "relative/path\n",
        "https://example.com/a b\n",
        "https://example.com/\0\n",
        "https://example.com/\t\n",
    ] {
        assert!(text.parse::<UrlFile>().is_err(), "{text:?}");
        assert!(text.parse::<UrlsFile>().is_err(), "{text:?}");
    }
    assert!("".parse::<UrlFile>().is_err());
    assert!(
        "https://one.example\nhttps://two.example\n"
            .parse::<UrlFile>()
            .is_err()
    );
    assert!(UrlsFile::new(["https://valid.example", "not-a-url"]).is_err());
    assert!(UrlFile::new("https://one.example\nhttps://two.example").is_err());
}

#[test]
fn updates_preserve_order_and_match_exact_url_spelling() {
    let mut file: UrlsFile = "https://b.example\nhttps://a.example\nhttps://b.example\n"
        .parse()
        .unwrap();
    let additions = UrlsFile::new([
        "https://a.example",
        "https://c.example",
        "https://c.example",
        "HTTPS://A.example",
    ])
    .unwrap();
    assert_eq!(file.append(&additions), 2);
    assert_eq!(
        file.to_string(),
        "https://b.example\nhttps://a.example\nhttps://b.example\nhttps://c.example\nHTTPS://A.example\n"
    );
    assert_eq!(file.append(&additions), 0);
    assert_eq!(
        file.remove(&UrlsFile::new(["https://b.example", "https://absent.example"]).unwrap()),
        2
    );
    assert_eq!(
        file.urls(),
        [
            "https://a.example",
            "https://c.example",
            "HTTPS://A.example"
        ]
    );
}
