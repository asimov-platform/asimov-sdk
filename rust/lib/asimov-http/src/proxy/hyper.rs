// This is free and unencumbered software released into the public domain.

use super::{Error, OriginProxy, bypasses_all};
use hyper_util::client::proxy::matcher::Matcher;
use url::Url;

impl OriginProxy {
    /// Returns this origin's effective endpoint for a Hyper connector (`hyper`).
    ///
    /// Applies NO_PROXY using the same matcher as reqwest, including the shared
    /// wildcard fix for IP literals. Native exclusions were resolved earlier.
    /// This does not perform backend environment/system discovery. The returned
    /// URL retains credentials; use it only for [`Self::origin`]. The caller
    /// owns CONNECT/SOCKS support, TLS, timeouts, and connection pooling.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidOrigin`] if the origin cannot be represented as
    /// an HTTP URI for matching.
    pub fn proxy_for_hyper(&self) -> Result<Option<&Url>, Error> {
        let Some(proxy) = self.proxy_url() else {
            return Ok(None);
        };
        if bypasses_all(self.no_proxy()) {
            return Ok(None);
        }
        let uri = self
            .origin()
            .ascii_serialization()
            .parse::<http::Uri>()
            .map_err(|_| Error::InvalidOrigin("origin cannot be represented as an HTTP URI"))?;
        // Use only the matcher's bypass parser. The sentinel is never connected
        // to; endpoint selection and credential validation belong to adapters.
        let matcher = Matcher::builder()
            .all("http://proxy.invalid")
            .no(self.no_proxy())
            .build();
        Ok(matcher.intercept(&uri).is_some().then_some(proxy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::{ProxyMode, ProxyOptions};

    #[test]
    fn bypass_rules_apply_before_exposing_an_endpoint() {
        for (target, bypass, direct) in [
            ("https://example.com/", ".EXAMPLE.COM", true),
            ("https://api.example.com/", "example.com", true),
            ("https://notexample.com/", "example.com", false),
            ("https://127.0.0.1/", "127.0.0.0/8", true),
            ("https://[::1]/", "::1", true),
            ("https://[2001:db8::1]/", "2001:db8::/32", true),
            ("https://127.0.0.1/", " * ", true),
            ("https://[::1]/", "*", true),
            ("https://localhost/", "127.0.0.0/8", false),
            ("https://example.com:8443/", "example.com:8443", false),
            ("https://example.com/", "", false),
        ] {
            for endpoint in ["http://user:pass@proxy.invalid", "socks5h://proxy.invalid"] {
                let target = Url::parse(target).unwrap();
                let policy = OriginProxy::resolve(
                    &target,
                    &ProxyOptions {
                        mode: ProxyMode::Explicit(endpoint.into()),
                        no_proxy: Some(bypass.into()),
                    },
                )
                .unwrap();
                assert_eq!(
                    policy.proxy_for_hyper().unwrap().is_none(),
                    direct,
                    "{target}, {bypass}"
                );
                if !direct {
                    assert_eq!(policy.proxy_for_hyper().unwrap(), policy.proxy_url());
                }
            }
        }
    }
}
