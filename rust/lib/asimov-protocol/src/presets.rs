// This is free and unencumbered software released into the public domain.

use iroh::endpoint::{Builder, presets::Preset};

#[cfg(all(feature = "std", not(target_family = "wasm")))]
mod pkarr;

/// The default preset for ASIMOV.
///
/// Native `std` builds use shared environment/native proxy discovery for Pkarr
/// address publication and resolution, with the endpoint's TLS and DNS settings.
/// GET redirects and method-preserving PUT redirects resolve each destination
/// independently (at most ten hops); HTTPS downgrades are rejected. Published
/// addresses remain relay-only, with periodic refresh and retries.
///
/// Relay connections and DNS address lookups use Iroh's transport configuration.
#[derive(Copy, Clone, Default, Debug)]
pub struct DefaultPreset;

impl Preset for DefaultPreset {
    fn apply(self, builder: Builder) -> Builder {
        #[cfg(all(feature = "std", not(target_family = "wasm")))]
        {
            use iroh::{address_lookup::DnsAddressLookup, endpoint};

            endpoint::presets::Minimal
                .apply(builder)
                .address_lookup(pkarr::Builder::default())
                .address_lookup(DnsAddressLookup::n0_dns())
                .relay_mode(endpoint::default_relay_mode())
        }
        #[cfg(not(all(feature = "std", not(target_family = "wasm"))))]
        iroh::endpoint::presets::N0.apply(builder)
    }
}
