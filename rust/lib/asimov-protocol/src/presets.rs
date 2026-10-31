// This is free and unencumbered software released into the public domain.

#[cfg(all(feature = "std", not(target_family = "wasm")))]
use alloc::vec::Vec;
use iroh::endpoint::{Builder, presets::Preset};

#[cfg(all(feature = "std", not(target_family = "wasm")))]
mod pkarr;
#[cfg(all(feature = "std", not(target_family = "wasm")))]
mod relay_proxy;

/// The default preset for ASIMOV.
///
/// Native `std` builds use shared environment/native proxy discovery for Pkarr
/// address publication and resolution, with the endpoint's TLS and DNS settings.
/// GET redirects and method-preserving PUT redirects resolve each destination
/// independently (at most ten hops); HTTPS downgrades are rejected. Published
/// addresses remain relay-only, with periodic refresh and retries.
///
/// Relay connections use the shared proxy policy when every default relay
/// resolves to the same HTTP(S) proxy, and connect directly otherwise,
/// including for SOCKS proxies. Iroh applies this proxy to all of its own HTTP
/// traffic, so it remains in effect if the relay mode is later replaced. DNS
/// address lookups use Iroh's transport configuration.
#[derive(Copy, Clone, Default, Debug)]
pub struct DefaultPreset;

impl Preset for DefaultPreset {
    fn apply(self, builder: Builder) -> Builder {
        #[cfg(all(feature = "std", not(target_family = "wasm")))]
        {
            use asimov_http::proxy::ProxyOptions;
            use iroh::{RelayUrl, address_lookup::DnsAddressLookup, endpoint};

            let relay_mode = endpoint::default_relay_mode();
            let relays = relay_mode.relay_map().urls::<Vec<RelayUrl>>();
            let proxy =
                relay_proxy::select(relays.into_iter().map(Into::into), &ProxyOptions::default());
            let builder = endpoint::presets::Minimal
                .apply(builder)
                .address_lookup(pkarr::Builder::default())
                .address_lookup(DnsAddressLookup::n0_dns())
                .relay_mode(relay_mode);
            match proxy {
                Some(proxy) => builder.proxy_url(proxy),
                None => builder,
            }
        }
        #[cfg(not(all(feature = "std", not(target_family = "wasm"))))]
        iroh::endpoint::presets::N0.apply(builder)
    }
}
