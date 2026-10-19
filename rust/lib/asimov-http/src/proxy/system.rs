// This is free and unencumbered software released into the public domain.

use super::{Error, ProxyMode, ProxyOptions, environment_proxy, parse_endpoint};
use alloc::string::String;
use core::fmt;
use url::{Origin, Url};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(windows, target_os = "linux", test))]
mod native;
#[cfg(windows)]
mod windows;

/// A proxy policy resolved for exactly one HTTP(S) origin (`system-proxy`).
///
/// Resolution snapshots the process environment and supported native settings.
/// Uppercase variables, including empty values, precede lowercase variants.
/// Per-scheme environment proxies precede native settings, then `ALL_PROXY`.
/// Nonempty environment overrides with unusable URLs skip native discovery and
/// fall back to a usable `ALL_PROXY`, or direct access. Explicit options are
/// validated instead. Presence of `REQUEST_METHOD` disables automatic selection.
///
/// Native discovery reads global macOS HTTP/HTTPS/SOCKS settings, Windows static
/// Internet Settings (including protocol maps and per-machine policy), and
/// GNOME/KDE desktop settings. Native bypass grammars remain distinct from
/// `NO_PROXY`. Empty local bypass overrides suppress native exclusions too.
/// macOS/Linux native SOCKS5 settings use proxy-side DNS. PAC/WPAD, saved
/// passwords, and per-interface/VPN settings are not resolved; portal-only
/// sources are rejected.
///
/// Native bypass decisions are evaluated for this origin. **Do not reuse the
/// resulting client for other origins or follow redirects automatically.** Use
/// [`Self::check_origin`] before dispatch if URLs are supplied dynamically. For
/// another origin, resolve a separate policy and configure a separate client.
/// The `reqwest` feature adds `configure_reqwest`; it disables redirects but
/// cannot restrict arbitrary requests made through a raw reqwest client.
///
/// This type does not configure service credentials, TLS trust, timeouts, or
/// request/response semantics. Debug output omits proxy URLs and credentials.
#[derive(Clone)]
pub struct OriginProxy {
    origin: Origin,
    pub(super) configuration: Configuration,
}

impl OriginProxy {
    /// Resolves proxy settings before constructing a client or accessing keys.
    ///
    /// # Errors
    ///
    /// Rejects non-HTTP(S) targets with [`Error::InvalidOrigin`]. Invalid
    /// explicit proxy URLs fail even when bypassed. Native read/configuration
    /// failures and unsupported resolver sources retain their proxy error.
    pub fn resolve(url: &Url, options: &ProxyOptions) -> Result<Self, Error> {
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(Error::InvalidOrigin("expected an HTTP(S) URL with a host"));
        }
        let configuration = resolve_options(
            options,
            || Environment::capture(url),
            |env| {
                #[cfg(target_os = "macos")]
                return macos::read(url, env);
                #[cfg(windows)]
                return windows::read(url, env);
                #[cfg(target_os = "linux")]
                return native::read_linux(url, env);
                #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
                {
                    let _ = env;
                    Ok(Route::Automatic)
                }
            },
        )?;
        Ok(Self {
            origin: url.origin(),
            configuration,
        })
    }

    /// The bound scheme, host, and effective port, without paths or credentials.
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The selected endpoint before applying [`Self::no_proxy`] rules.
    ///
    /// `None` selects direct access. `Some` is a candidate: adapters must still
    /// apply the NO_PROXY list, including `*` for IP literals. Native exclusions
    /// have already been evaluated for this origin. The URL may contain proxy
    /// credentials; do not log it without redaction.
    pub fn proxy_url(&self) -> Option<&Url> {
        self.configuration.proxy.as_ref()
    }

    /// Environment-syntax bypass rules to apply to [`Self::proxy_url`].
    ///
    /// This is the effective inherited or explicitly replaced `NO_PROXY` list;
    /// native exclusion syntax is never copied into it. The reqwest adapter
    /// applies it automatically. Other adapters must preserve its semantics.
    pub fn no_proxy(&self) -> &str {
        &self.configuration.no_proxy
    }

    /// Checks a destination before dispatch through an origin-scoped client.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidOrigin`] if scheme, host, or effective port differs.
    pub fn check_origin(&self, url: &Url) -> Result<(), Error> {
        if url.origin() != self.origin {
            return Err(Error::InvalidOrigin(
                "request must use the configured origin",
            ));
        }
        Ok(())
    }
}

impl fmt::Debug for OriginProxy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OriginProxy")
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct Environment {
    proxy: String,
    all_proxy: String,
    no_proxy: String,
    no_proxy_override: bool,
    cgi: bool,
}

impl Environment {
    fn capture(url: &Url) -> Self {
        fn first(upper: &str, lower: &str) -> String {
            std::env::var(upper)
                .or_else(|_| std::env::var(lower))
                .unwrap_or_default()
        }
        Self {
            proxy: match url.scheme() {
                "https" => first("HTTPS_PROXY", "https_proxy"),
                _ => first("HTTP_PROXY", "http_proxy"),
            },
            all_proxy: first("ALL_PROXY", "all_proxy"),
            no_proxy: first("NO_PROXY", "no_proxy"),
            no_proxy_override: false,
            cgi: std::env::var_os("REQUEST_METHOD").is_some(),
        }
    }

    fn use_native_bypass(&self) -> bool {
        !self.no_proxy_override && self.no_proxy.is_empty()
    }
}

#[derive(Debug, PartialEq)]
enum Route {
    Automatic,
    Direct,
    Native(Url),
}

#[derive(Clone)]
pub(super) struct Configuration {
    pub(super) proxy: Option<Url>,
    pub(super) no_proxy: String,
}

fn resolve_options(
    options: &ProxyOptions,
    capture: impl FnOnce() -> Environment,
    native: impl FnOnce(&Environment) -> Result<Route, Error>,
) -> Result<Configuration, Error> {
    match &options.mode {
        ProxyMode::Direct => Ok(Configuration {
            proxy: None,
            no_proxy: String::new(),
        }),
        ProxyMode::Explicit(input) => {
            let proxy = parse_endpoint(input)?;
            let no_proxy = options
                .no_proxy
                .clone()
                .unwrap_or_else(|| capture().no_proxy);
            Ok(Configuration {
                proxy: Some(proxy),
                no_proxy,
            })
        },
        ProxyMode::Automatic => {
            let mut env = capture();
            if let Some(no_proxy) = &options.no_proxy {
                env.no_proxy.clone_from(no_proxy);
                env.no_proxy_override = true;
            }
            resolve_environment(&env, || native(&env))
        },
    }
}

fn select(
    env: &Environment,
    native: impl FnOnce() -> Result<Route, Error>,
) -> Result<Route, Error> {
    if env.cgi || super::bypasses_all(&env.no_proxy) {
        Ok(Route::Direct)
    } else if !env.proxy.is_empty() {
        // Preserve the legacy ALL_PROXY fallback for unusable environment URLs.
        Ok(Route::Automatic)
    } else {
        native()
    }
}

fn resolve_environment(
    env: &Environment,
    native: impl FnOnce() -> Result<Route, Error>,
) -> Result<Configuration, Error> {
    let proxy = match select(env, native)? {
        Route::Automatic => {
            environment_proxy(&env.proxy).or_else(|| environment_proxy(&env.all_proxy))
        },
        Route::Direct => None,
        Route::Native(proxy) => Some(proxy),
    };
    Ok(Configuration {
        proxy,
        no_proxy: env.no_proxy.clone(),
    })
}

#[cfg(test)]
mod tests;
