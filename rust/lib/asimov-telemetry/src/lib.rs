// This is free and unencumbered software released into the public domain.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

use alloc::{string::String, vec::Vec};
use core::time::Duration;
use serde::Serialize;

#[cfg(feature = "std")]
mod client;
#[cfg(feature = "std")]
pub use client::{Telemetry, disable, enable, is_disabled};

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Fetch,
    Read,
    List,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Failure,
}

#[derive(Clone, Debug)]
pub enum Event {
    Installed,
    CommandStarted {
        command: String,
        modules: Vec<String>,
    },
    CommandFinished {
        command: String,
        modules: Vec<String>,
        exit_code: i32,
        duration: Duration,
    },
    ModuleOperationFinished {
        operation: Operation,
        module: String,
        outcome: Outcome,
        duration: Duration,
    },
}
