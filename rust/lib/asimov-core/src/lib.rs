// This is free and unencumbered software released into the public domain.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub mod env;

pub mod error;
pub use error::*;

pub use ::dogma::traits::*;

#[doc(hidden)]
pub mod crates {
    #[cfg(feature = "std")]
    pub use ::cap_directories;
    #[cfg(feature = "std")]
    pub use ::cap_std;
    pub use ::dogma;
    #[cfg(feature = "serde")]
    pub use ::serde;
}

#[cfg(feature = "tracing")]
#[doc(hidden)]
mod tracing {
    pub use tracing::{debug, error, info, trace, warn};
}

#[cfg(not(feature = "tracing"))]
#[doc(hidden)]
#[rustfmt::skip]
mod tracing {
    // These macros are fallback implementations used when the `tracing` feature is disabled.
    // They are no-op definitions to ensure that code using these macros compiles without errors.
    #[macro_export] macro_rules! debug { ($($arg:tt)+) => (); }
    #[macro_export] macro_rules! error { ($($arg:tt)+) => (); }
    #[macro_export] macro_rules! info { ($($arg:tt)+) => (); }
    #[macro_export] macro_rules! trace { ($($arg:tt)+) => (); }
    #[macro_export] macro_rules! warn { ($($arg:tt)+) => (); }
}

#[allow(unused)]
pub use tracing::*;

mod module_name;
pub use module_name::*;

mod filename;
pub use filename::*;
