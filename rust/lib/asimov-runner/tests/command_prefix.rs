// This is free and unencumbered software released into the public domain.

#![cfg(unix)]

extern crate alloc;

use alloc::vec::Vec;
use asimov_runner::{
    Fetcher, FetcherOptions, GraphOutput, Lister, ListerCapabilities, ListerOptions, StreamExt,
};

#[tokio::test]
async fn prefixes_precede_generated_options_and_survive_reconfiguration() {
    let script = "printf '%s\\n' \"$@\"";
    let mut fetcher = Fetcher::new_with_args(
        "/bin/sh",
        ["-c", script, "fixture", "fetch"],
        "https://example.com/a b",
        GraphOutput::Captured,
        FetcherOptions::builder().output("jsonl").build(),
    );
    let mut output = fetcher.execute().await.unwrap();
    let mut bytes = Vec::new();
    while let Some(batch) = output.next().await {
        for line in batch.unwrap().lines() {
            bytes.extend_from_slice(line);
        }
    }
    assert_eq!(bytes, b"fetch\n--output=jsonl\nhttps://example.com/a b\n");

    let mut lister: Lister = Lister::new_with_args(
        "/bin/sh",
        ["-c", script, "fixture", "list"],
        "urn:collection",
        GraphOutput::Captured,
        ListerOptions::builder().offset(2).output("jsonl").build(),
    )
    .with_capabilities(ListerCapabilities::default());
    let mut output = lister.execute().await.unwrap();
    bytes.clear();
    while let Some(batch) = output.next().await {
        for line in batch.unwrap().lines() {
            bytes.extend_from_slice(line);
        }
    }
    assert_eq!(bytes, b"list\n--offset=2\n--output=jsonl\nurn:collection\n");
}

#[tokio::test]
async fn environment_variables_reach_the_subprocess_and_survive_reconfiguration() {
    let script = "printf '%s\\n' \"$ASIMOV_RUNNER_TEST\"";
    let mut fetcher = Fetcher::new_with_args(
        "/bin/sh",
        ["-c", script],
        "urn:resource",
        GraphOutput::Captured,
        FetcherOptions::default(),
    )
    .with_env("ASIMOV_RUNNER_TEST", "fetched");
    let mut output = fetcher.execute().await.unwrap();
    let mut bytes = Vec::new();
    while let Some(batch) = output.next().await {
        for line in batch.unwrap().lines() {
            bytes.extend_from_slice(line);
        }
    }
    assert_eq!(bytes, b"fetched\n");

    let mut lister: Lister = Lister::new_with_args(
        "/bin/sh",
        ["-c", script],
        "urn:collection",
        GraphOutput::Captured,
        ListerOptions::default(),
    )
    .with_env("ASIMOV_RUNNER_TEST", "listed")
    .with_capabilities(ListerCapabilities::default());
    let mut output = lister.execute().await.unwrap();
    bytes.clear();
    while let Some(batch) = output.next().await {
        for line in batch.unwrap().lines() {
            bytes.extend_from_slice(line);
        }
    }
    assert_eq!(bytes, b"listed\n");
}
