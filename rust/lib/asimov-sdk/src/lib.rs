// This is free and unencumbered software released into the public domain.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "config")]
pub use asimov_config as config;

#[cfg(feature = "core")]
pub use asimov_core as core;

#[cfg(feature = "credit")]
pub use asimov_credit as credit;

/// State-directory traits, plus filesystem implementations with `std` enabled.
///
/// ```
/// use asimov_sdk::directory::StateDirectory;
///
/// fn has_modules(directory: &impl StateDirectory) -> bool {
///     directory.has_modules()
/// }
/// ```
#[cfg(feature = "directory")]
pub use asimov_directory as directory;

#[cfg(feature = "env")]
pub use asimov_env as env;

#[cfg(feature = "flow")]
pub use asimov_flow as flow;

#[cfg(feature = "id")]
pub use asimov_id as id;

/// Shared HTTP integrity and optional client proxy support.
///
/// `std,http` includes signatures and body adapters. `http-proxy` exposes
/// alloc-only proxy options; `http-client` adds native discovery and the reqwest
/// adapter. With defaults disabled, those proxy flags need not enable the SDK's
/// `std`/integrity bundle.
///
/// ```
/// # #[cfg(feature = "http-proxy")] {
/// use asimov_sdk::http::proxy::{ProxyMode, ProxyOptions};
/// let options = ProxyOptions {
///     mode: ProxyMode::Direct,
///     ..ProxyOptions::default()
/// };
/// assert_eq!(options.mode, ProxyMode::Direct);
/// # }
/// ```
#[cfg(feature = "http")]
pub use asimov_http as http;

#[cfg(feature = "installer")]
pub use asimov_installer as installer;

#[cfg(feature = "kb")]
pub use asimov_kb as kb;

#[cfg(feature = "module")]
pub use asimov_module as module;

#[cfg(feature = "patterns")]
pub use asimov_patterns as patterns;

#[cfg(feature = "prompt")]
pub use asimov_prompt as prompt;

#[cfg(feature = "proxy")]
pub use asimov_proxy as proxy;

#[cfg(feature = "registry")]
pub use asimov_registry as registry;

#[cfg(feature = "runner")]
pub use asimov_runner as runner;

/// Remote pattern execution and owned request bodies.
///
/// Enable `remote` and `jcs` for RFC 8785 canonical request serialization.
///
/// ```
/// # #[cfg(feature = "jcs")] {
/// use asimov_sdk::remote::{FetcherRequest, ListerRequest};
///
/// let fetch = FetcherRequest::new("example:", Default::default());
/// assert_eq!(
///     fetch.to_canonical_json_bytes().unwrap(),
///     br#"{"urls":["example:"]}"#,
/// );
/// let list: ListerRequest = ListerRequest::new("example:", Default::default());
/// assert_eq!(list.to_canonical_json().unwrap(), r#"{"url":"example:"}"#);
/// # }
/// ```
#[cfg(feature = "remote")]
pub use asimov_remote as remote;

#[cfg(feature = "snapshot")]
pub use asimov_snapshot as snapshot;

#[cfg(feature = "telemetry")]
pub use asimov_telemetry as telemetry;
