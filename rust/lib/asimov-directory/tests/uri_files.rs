// This is free and unencumbered software released into the public domain.

extern crate alloc;

use alloc::string::ToString;
use asimov_directory::{UriFile, UrisFile};

#[test]
fn formats_round_trip_without_normalizing_uris() {
    let text = "HTTPS://例え.テスト/🦀/%2f?q=%41#片\n";
    let file: UriFile = text.parse().unwrap();
    assert_eq!(file.uri(), text.trim_end_matches('\n'));
    assert_eq!(file.to_string(), text);
    let text = "HTTPS://例え.テスト/🦀#片\nurn:isbn:9780140328721\nfile:///tmp/a%20b\n";
    let file: UrisFile = text.parse().unwrap();
    assert_eq!(file.to_string(), text);
    assert_eq!("".parse::<UrisFile>().unwrap(), UrisFile::default());
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
        "# comment\n",
        "#https://example.com\nhttps://example.com\n",
    ] {
        assert!(text.parse::<UriFile>().is_err(), "{text:?}");
        assert!(text.parse::<UrisFile>().is_err(), "{text:?}");
    }
    assert!("".parse::<UriFile>().is_err());
    assert!(
        "https://one.example\nhttps://two.example\n"
            .parse::<UriFile>()
            .is_err()
    );
    assert!(UrisFile::new(["https://valid.example", "not-a-uri"]).is_err());
    assert!(UriFile::new("https://one.example\nhttps://two.example").is_err());
}

#[test]
fn updates_preserve_order_and_match_exact_uri_spelling() {
    let mut file: UrisFile = "https://b.example\nhttps://a.example\nhttps://b.example\n"
        .parse()
        .unwrap();
    let additions = UrisFile::new([
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
        file.remove(&UrisFile::new(["https://b.example", "https://absent.example"]).unwrap()),
        2
    );
    assert_eq!(
        file.uris(),
        [
            "https://a.example",
            "https://c.example",
            "HTTPS://A.example"
        ]
    );
}
