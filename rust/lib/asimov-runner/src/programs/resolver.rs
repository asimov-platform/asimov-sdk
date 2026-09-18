// This is free and unencumbered software released into the public domain.

//! URI-to-URL resolution through an external resolver program.
//!
//! Captured stdout is parsed as UTF-8 absolute URLs, one per line.

use crate::{Executor, ExecutorError, Input, Output};
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, process::Stdio};

pub use asimov_patterns::ResolverOptions;

/// A list of resolved URLs, or an execution error.
///
/// Captured URLs retain their original spelling, order, and duplicates. Ignored,
/// inherited, or forwarded stdout produces an empty vector.
pub type ResolverResult = std::result::Result<Vec<String>, ExecutorError>;

/// An external [resolver] that maps a URI (a URN or URL) to resolved URLs.
///
/// The input URI is passed as one command-line argument, and stdin is connected
/// to the null device. The external program performs resolution; captured stdout
/// is buffered and parsed after successful completion.
/// Stream handling follows the behavior described in [`crate::programs`].
/// Use [`Output::Captured`] to receive URLs; other output policies route the bytes
/// without parsing and return an empty vector.
///
/// The specification requires the external program to emit UTF-8 absolute URLs,
/// one per LF-terminated line, with no blank records. A host parsing that output
/// must also accept CRLF and an unterminated final nonempty line while preserving
/// order and all content except line terminators. This wrapper rejects blank
/// records, whitespace/control characters, invalid UTF-8, and URLs rejected by
/// [`url::Url::parse`]. Parsed URLs are validated but never normalized in the result.
///
/// [resolver]: https://asimov-specs.github.io/program-patterns/#resolver
#[allow(unused)]
#[derive(Debug)]
pub struct Resolver {
    executor: Executor,
    options: ResolverOptions,
    input: String,
    output: Output,
}

impl Resolver {
    /// Configures a resolver for the URI `input` without starting it.
    ///
    /// Adds `--limit=<limit>` when `options.limit` is set, followed by
    /// `options.other` and the unvalidated URI as a single argument. `output`
    /// selects stdout handling; stderr is captured for failure diagnostics.
    pub fn new(
        program: impl AsRef<OsStr>,
        input: impl AsRef<str>,
        output: Output,
        options: ResolverOptions,
    ) -> Self {
        let input = input.as_ref().to_string();
        let mut executor = Executor::new(program);
        executor
            .command()
            .args(if let Some(limit) = options.limit {
                vec![format!("--limit={}", limit)]
            } else {
                vec![]
            })
            .args(&options.other)
            .arg(&input)
            .stdin(Stdio::null())
            .stdout(output.as_stdio())
            .stderr(Stdio::piped());

        Self {
            executor,
            options,
            input,
            output,
        }
    }

    /// Runs a resolver and parses captured stdout into an ordered URL list.
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] for spawning, forwarding, waiting, unsuccessful
    /// exit, or malformed captured output. Parsing failures are I/O `InvalidData`
    /// errors wrapped in [`ExecutorError::UnexpectedOther`].
    pub async fn execute(&mut self) -> ResolverResult {
        let stdout = self
            .executor
            .execute_with_io(&mut Input::Ignored, &mut self.output)
            .await?;
        parse_urls(&stdout.into_inner())
    }
}

fn parse_urls(bytes: &[u8]) -> ResolverResult {
    use std::io::{Error, ErrorKind};
    let text =
        std::str::from_utf8(bytes).map_err(|error| Error::new(ErrorKind::InvalidData, error))?;
    text.split_inclusive('\n')
        .enumerate()
        .map(|(index, record)| {
            let value = match record.strip_suffix('\n') {
                Some(line) => line.strip_suffix('\r').unwrap_or(line),
                None => record,
            };
            if value.is_empty()
                || value.chars().any(|c| c.is_whitespace() || c.is_control())
                || url::Url::parse(value).is_err()
            {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("invalid URL on resolver output line {}", index + 1),
                )
                .into());
            }
            Ok(value.to_string())
        })
        .collect()
}

impl asimov_patterns::Resolver<Vec<String>> for Resolver {}

#[async_trait]
impl asimov_patterns::Execute<Vec<String>> for Resolver {
    type Error = ExecutorError;

    async fn execute(&mut self) -> ResolverResult {
        self.execute().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_urls_without_normalization() {
        let urls = parse_urls(
            b"HTTPS://Example.COM/a%2fb?q=x,y\r\nhttps://example.com/\nhttps://example.com/",
        )
        .unwrap();
        assert_eq!(
            urls,
            [
                "HTTPS://Example.COM/a%2fb?q=x,y",
                "https://example.com/",
                "https://example.com/"
            ]
        );
        assert!(parse_urls(b"").unwrap().is_empty());
    }

    #[test]
    fn rejects_malformed_records() {
        for bytes in [
            b"\n".as_slice(),
            b"\r\n",
            b"https://example.com/\n\n",
            b"relative/path",
            b" https://example.com/",
            b"https://example.com/ ",
            b"https://example.com/a b",
            b"https://example.com/\t",
            b"https://example.com/\r",
            b"https://example.com/\0",
            b"\xff",
        ] {
            assert!(
                matches!(parse_urls(bytes), Err(ExecutorError::UnexpectedOther(error))
                if error.kind() == std::io::ErrorKind::InvalidData),
                "{bytes:?}"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_execute() {
        let mut resolver = Resolver::new(
            "/bin/sh",
            "printf 'https://example.com/\\r\\nhttps://example.net/'",
            Output::Captured,
            ResolverOptions::builder().other("-c").build(),
        );
        assert_eq!(
            resolver.execute().await.unwrap(),
            ["https://example.com/", "https://example.net/"]
        );

        let mut resolver = Resolver::new(
            "/bin/sh",
            "printf 'not a URL'",
            Output::Ignored,
            ResolverOptions::builder().other("-c").build(),
        );
        assert!(resolver.execute().await.unwrap().is_empty());

        let mut resolver = Resolver::new(
            "/bin/sh",
            "printf 'not a URL'; printf 'failed' >&2; exit 65",
            Output::Captured,
            ResolverOptions::builder().other("-c").build(),
        );
        assert!(matches!(resolver.execute().await,
            Err(ExecutorError::Failure(_, Some(stderr))) if stderr == "failed"));
    }
}
