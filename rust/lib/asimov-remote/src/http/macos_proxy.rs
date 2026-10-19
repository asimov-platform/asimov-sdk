// This is free and unencumbered software released into the public domain.

//! Construction-time routing for the executor's single, immutable origin.
//!
//! Use ordinary reqwest proxies: its custom callback cannot report errors and
//! ignores attached `NoProxy` rules. Discovery and execution use the same origin,
//! and redirects are disabled, so native bypass decisions can be made once.

use super::{
    Error,
    proxy::{Environment, Route},
};
use alloc::{format, string::String, vec, vec::Vec};
use core::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use plist::{Dictionary, Value};
use reqwest::Url;
use system_configuration::{
    core_foundation::{
        base::{CFType, TCFType},
        dictionary::CFDictionary,
        propertylist::{create_data, kCFPropertyListBinaryFormat_v1_0},
        string::CFString,
    },
    dynamic_store::SCDynamicStoreBuilder,
};

pub(super) fn read(url: &Url, env: &Environment) -> Result<Route, Error> {
    select(url, env, read_system)
}

fn select(
    url: &Url,
    env: &Environment,
    read: impl FnOnce() -> Result<Option<Dictionary>, Error>,
) -> Result<Route, Error> {
    super::proxy::select(env, || select_native(url, env, read))
}

fn select_native(
    url: &Url,
    env: &Environment,
    read: impl FnOnce() -> Result<Option<Dictionary>, Error>,
) -> Result<Route, Error> {
    let Some(settings) = read()? else {
        return Ok(Route::Automatic);
    };
    let (enabled, host, port) = match url.scheme() {
        "http" => ("HTTPEnable", "HTTPProxy", "HTTPPort"),
        "https" => ("HTTPSEnable", "HTTPSProxy", "HTTPSPort"),
        _ => return Ok(Route::Automatic),
    };
    let proxy = if flag(settings.get(enabled)) == Some(true) {
        proxy_url(&settings, host, port, "http", 80)?
    } else if flag(settings.get("SOCKSEnable")) == Some(true) {
        proxy_url(&settings, "SOCKSProxy", "SOCKSPort", "socks5h", 1080)?
    } else {
        return Ok(Route::Automatic);
    };
    if env.no_proxy.is_empty() && bypasses(&settings, url) {
        Ok(Route::Direct)
    } else {
        Ok(Route::Native(proxy))
    }
}

fn read_system() -> Result<Option<Dictionary>, Error> {
    let Some(settings) = SCDynamicStoreBuilder::new("asimov-remote")
        .build()
        .and_then(|store| store.get_proxies())
    else {
        // The binding cannot distinguish missing settings from a failed read.
        return Ok(None);
    };
    snapshot(&settings).map(Some)
}

fn snapshot(settings: &CFDictionary<CFString, CFType>) -> Result<Dictionary, Error> {
    // core-foundation deliberately disallows typed CFArray downcasts. Its safe
    // plist serialization API lets us inspect heterogeneous values without raw
    // pointer casts, and all Core Foundation objects stay on this thread.
    let data = create_data(settings.as_CFTypeRef(), kCFPropertyListBinaryFormat_v1_0)
        .map_err(|_| Error::InvalidProxy("SystemConfiguration"))?;
    Value::from_reader(std::io::Cursor::new(data.bytes()))
        .ok()
        .and_then(Value::into_dictionary)
        .ok_or(Error::InvalidProxy("SystemConfiguration"))
}

fn flag(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Boolean(value) => Some(*value),
        Value::Integer(value) => match value.as_signed() {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        },
        _ => None,
    }
}

fn proxy_url(
    settings: &Dictionary,
    host_key: &'static str,
    port_key: &'static str,
    scheme: &str,
    default_port: u16,
) -> Result<Url, Error> {
    let host = settings
        .get(host_key)
        .and_then(Value::as_string)
        .filter(|host| !host.is_empty() && host.trim() == *host)
        .ok_or(Error::InvalidProxy(host_key))?;
    // These fields contain hostnames, not URLs, credentials, or inline ports.
    if host.contains(['/', '\\', '@', '?', '#', '%']) || host.chars().any(char::is_control) {
        return Err(Error::InvalidProxy(host_key));
    }
    let authority = if let Ok(ip) = host.parse::<Ipv6Addr>() {
        format!("[{ip}]")
    } else {
        if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
            return Err(Error::InvalidProxy(host_key));
        }
        String::from(host)
    };
    // HTTPSProxy selects HTTPS destinations, but speaks HTTP CONNECT to the
    // proxy. TLS to a proxy is selected by an explicit environment URL instead.
    let mut proxy = Url::parse(&format!("{scheme}://{authority}/"))
        .map_err(|_| Error::InvalidProxy(host_key))?;
    let port = match settings.get(port_key) {
        None => default_port,
        Some(value) => value
            .as_unsigned_integer()
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port != 0)
            .ok_or(Error::InvalidProxy(port_key))?,
    };
    proxy
        .set_port(Some(port))
        .map_err(|_| Error::InvalidProxy(port_key))?;
    Ok(proxy)
}

fn bypasses(settings: &Dictionary, url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    let ip = host.parse::<IpAddr>().ok();
    let simple = flag(settings.get("ExcludeSimpleHostnames"));
    let exceptions = settings.get("ExceptionsList").and_then(Value::as_array);
    // Native loopback bypass is enabled by the presence of a bypass setting,
    // including an empty ExceptionsList. It is not the whole 127.0.0.0/8 range.
    if (simple.is_some() || exceptions.is_some())
        && (host.eq_ignore_ascii_case("localhost")
            || ip == Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
            || ip == Some(IpAddr::V6(Ipv6Addr::LOCALHOST)))
    {
        return true;
    }
    // Interpret the enable flag explicitly and exclude IP literals from the
    // simple *hostname* rule, including IPv6 addresses which contain no dots.
    if simple == Some(true) && ip.is_none() && !host.contains('.') {
        return true;
    }
    exceptions.is_some_and(|entries| {
        entries
            .iter()
            .filter_map(Value::as_string)
            .any(|rule| matches(rule, host))
    })
}

fn matches(rule: &str, host: &str) -> bool {
    if let Some((network, prefix)) = rule.split_once('/') {
        // Native IPv4 ranges allow abbreviated networks, e.g. 169.254/16.
        // Match URL literals only: a bypass check must not cause a DNS lookup.
        if !prefix.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        let (Ok(host), Ok(prefix)) = (host.parse::<Ipv4Addr>(), prefix.parse::<u32>()) else {
            return false;
        };
        if prefix > 32 {
            return false;
        }
        let mut octets = [0; 4];
        for (index, part) in network.split('.').enumerate() {
            let Some(octet) = octets.get_mut(index) else {
                return false;
            };
            let Ok(value) = part.parse::<u8>() else {
                return false;
            };
            if !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
            {
                return false;
            }
            *octet = value;
        }
        let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
        return host.to_bits() & mask == Ipv4Addr::from(octets).to_bits() & mask;
    }
    let rule = rule.strip_suffix('.').unwrap_or(rule);
    let host = host.strip_suffix('.').unwrap_or(host);
    if rule.eq_ignore_ascii_case(host) {
        return true;
    }
    if !rule.contains('.') {
        return false;
    }
    let labels: Vec<_> = host.split('.').collect();
    let mut previous = vec![false; labels.len() + 1];
    previous[0] = true;
    // Wildcards occupy whole labels. A leading wildcard/leading dot requires a
    // subdomain; elsewhere '*' can span zero or more labels. Dynamic programming
    // bounds matching work even for patterns containing many wildcards.
    for (index, label) in rule.split('.').enumerate() {
        let mut next = vec![false; previous.len()];
        if label == "*" || (index == 0 && label.is_empty()) {
            for consumed in 0..next.len() {
                next[consumed] = if index == 0 {
                    consumed > 0 && previous[consumed - 1]
                } else {
                    previous[consumed]
                } || (consumed > 0 && next[consumed - 1]);
            }
        } else if label.is_empty() || label.contains('*') {
            return false;
        } else {
            for consumed in 1..next.len() {
                next[consumed] =
                    previous[consumed - 1] && label.eq_ignore_ascii_case(labels[consumed - 1]);
            }
        }
        previous = next;
    }
    previous[labels.len()]
}

#[cfg(test)]
mod tests;
