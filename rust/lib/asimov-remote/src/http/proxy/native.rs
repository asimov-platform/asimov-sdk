// This is free and unencumbered software released into the public domain.

//! Adapt static desktop settings without adopting proxy-watch's environment
//! precedence, NO_PROXY grammar, or implicit environment loopback exclusions.

use super::{Environment, Error, Route};
use proxy_watch::{ProxyEntry, ProxyMode, ProxyScheme, ProxyStep, Scheme};
use reqwest::Url;

pub(super) fn route(
    mode: &ProxyMode,
    url: &Url,
    env: &Environment,
    setting: &'static str,
) -> Result<Route, Error> {
    let scheme = match url.scheme() {
        "https" => Scheme::Https,
        _ => Scheme::Http,
    };
    let endpoint = match mode.entry_for(scheme) {
        Some(ProxyEntry::Use(endpoint)) => endpoint,
        Some(ProxyEntry::Unusable(_)) => return Err(Error::InvalidProxy(setting)),
        // No static proxy in this slot: ALL_PROXY may still supply one.
        _ => return Ok(Route::Automatic),
    };
    let step = match endpoint.scheme_hint {
        None | Some(ProxyScheme::Http) => ProxyStep::Http(endpoint.clone()),
        Some(ProxyScheme::Https) => ProxyStep::Https(endpoint.clone()),
        Some(ProxyScheme::Socks4 | ProxyScheme::Socks4a) => ProxyStep::Socks4(endpoint.clone()),
        Some(ProxyScheme::Socks5 | ProxyScheme::Socks5h) => ProxyStep::Socks5(endpoint.clone()),
        _ => return Err(Error::InvalidProxy(setting)),
    };
    let proxy = step.to_url().ok_or(Error::InvalidProxy(setting))?;
    if env.use_native_bypass() && mode.bypass().is_some_and(|bypass| bypass.matches_url(url)) {
        Ok(Route::Direct)
    } else {
        Ok(Route::Native(proxy))
    }
}

#[cfg(target_os = "linux")]
pub(super) fn read_linux(url: &Url, env: &Environment) -> Result<Route, Error> {
    use proxy_watch::ProxyConfigSource;

    let config = match proxy_watch::read() {
        Ok(config) => config,
        // Headless systems need neither a desktop nor GLib installed.
        Err(proxy_watch::Error::Unsupported) => return Ok(Route::Automatic),
        Err(_) => return Err(Error::InvalidProxy("Linux desktop settings")),
    };
    if config
        .sources
        .first()
        .is_some_and(|(source, _)| *source == ProxyConfigSource::Portal)
    {
        // The library's portal snapshot resolves a fixed probe URL. It cannot
        // establish the route for this origin, especially under host-based PAC.
        return Err(Error::UnsupportedProxy("Linux desktop portal"));
    }
    let mut selected = route(&config.effective, url, env, "Linux desktop proxy")?;
    // Desktop SOCKS5 settings use proxy-side destination lookup. Reqwest uses
    // local DNS for its socks5:// spelling, unlike GLib/Qt's native transport.
    if let Route::Native(proxy) = &mut selected
        && proxy.scheme() == "socks5"
    {
        proxy
            .set_scheme("socks5h")
            .map_err(|_| Error::InvalidProxy("Linux SOCKS proxy"))?;
    }
    Ok(selected)
}

#[cfg(test)]
mod tests;
