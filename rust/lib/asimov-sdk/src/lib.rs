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

#[cfg(feature = "env")]
pub use asimov_env as env;

#[cfg(feature = "flow")]
pub use asimov_flow as flow;

#[cfg(feature = "id")]
pub use asimov_id as id;

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

#[cfg(feature = "registry")]
pub use asimov_registry as registry;

#[cfg(feature = "runner")]
pub use asimov_runner as runner;

#[cfg(feature = "remote")]
pub use asimov_remote as remote;

#[cfg(feature = "snapshot")]
pub use asimov_snapshot as snapshot;
