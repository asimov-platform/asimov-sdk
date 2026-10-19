// This is free and unencumbered software released into the public domain.

use super::*;

#[test]
fn explicit_endpoints_validate_without_ambient_settings() {
    for (input, scheme, host) in [
        ("proxy.invalid:3128", "http", "proxy.invalid"),
        ("http://proxy.invalid", "http", "proxy.invalid"),
        ("https://proxy.invalid:8443/", "https", "proxy.invalid"),
        ("socks4://127.0.0.1", "socks4", "127.0.0.1"),
        ("socks4a://proxy.invalid:1080", "socks4a", "proxy.invalid"),
        ("socks5://[::1]:1080", "socks5", "[::1]"),
        (
            "socks5h://User%40Realm:p%3Aa%25ss+@proxy.invalid",
            "socks5h",
            "proxy.invalid",
        ),
    ] {
        let proxy = parse_endpoint(input).unwrap();
        assert_eq!(proxy.scheme(), scheme);
        assert_eq!(proxy.host_str(), Some(host));
    }
    for input in [
        "",
        " ",
        "http://",
        "http://[::1",
        "http://proxy.invalid:65536",
        "http://proxy.invalid:0",
        "socks5://proxy.invalid:0",
        "ftp://User:Secret@proxy.invalid",
        "socks://proxy.invalid",
        "file:///proxy.invalid",
        "http://User:Secret@proxy.invalid/path",
        "http://proxy.invalid?Secret",
        "https://proxy.invalid#Secret",
        "http://User:Secret@proxy.invalid:bad",
        " http://proxy.invalid",
        "http://proxy.invalid\n",
        "http://user:sec\tret@proxy.invalid",
        "socks5h://bücher.invalid",
        "socks5h://b%C3%BCcher.invalid",
    ] {
        let error = parse_endpoint(input).expect_err(input);
        assert!(matches!(error, Error::InvalidProxyUrl(_)));
        assert!(!format!("{error:?}: {error}").contains("Secret"));
        assert!(core::error::Error::source(&error).is_none());
    }
}

#[test]
fn option_debug_output_omits_even_malformed_proxy_credentials() {
    for endpoint in [
        "http://PrivateUser:PrivatePassword@proxy.invalid:3128",
        "invalid://PrivateUser:PrivatePassword@proxy.invalid:bad",
    ] {
        let options = ProxyOptions {
            mode: ProxyMode::Explicit(endpoint.into()),
            no_proxy: None,
        };
        let debug = format!("{options:?}");
        assert!(debug.contains("Explicit([REDACTED])"));
        assert!(!debug.contains("PrivateUser"));
        assert!(!debug.contains("PrivatePassword"));
    }
}

#[test]
fn environment_urls_keep_transport_dns_modes_and_credentials() {
    for scheme in ["http", "https", "socks4", "socks4a", "socks5", "socks5h"] {
        let url = environment_proxy(&format!(
            "{scheme}://User%40Realm:p%3Aa%25ss+@proxy.invalid:1080"
        ))
        .unwrap();
        assert_eq!(url.scheme(), scheme);
        assert_eq!(url.username(), "User%40Realm");
        assert_eq!(url.password(), Some("p%3Aa%25ss+"));
    }
    assert_eq!(
        environment_proxy("proxy.invalid:3128").unwrap().scheme(),
        "http"
    );
    for value in [
        "",
        "ftp://proxy.invalid",
        "http://proxy.invalid:bad",
        "http://",
    ] {
        assert!(environment_proxy(value).is_none());
    }
}
