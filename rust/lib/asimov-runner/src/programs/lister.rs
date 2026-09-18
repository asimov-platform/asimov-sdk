// This is free and unencumbered software released into the public domain.

//! URL-based directory iteration through an external lister program.

use crate::{CommandExt, Executor, ExecutorError, GraphOutput, JsonlStream, OptionSupport};
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
};
use async_trait::async_trait;
use derive_more::Debug;
use futures_lite::StreamExt;
use std::{ffi::OsStr, process::Stdio};
use tokio::io::{AsyncWrite, AsyncWriteExt};

pub use asimov_patterns::{ListerCapabilities, ListerOptions};

/// A live stream of JSONL lines from a [`Lister`].
///
/// Lines retain their LF or CRLF terminators; a final unterminated line is also
/// yielded. Bytes are neither decoded nor validated as JSON. Read, wait, and
/// unsuccessful-exit errors are yielded as a final error item. Consume the stream
/// to completion to check process success, even when stdout is not captured,
/// unless the configured line limit is reached. Reaching that limit intentionally
/// stops the child and ends the stream without checking its eventual exit status.
/// Dropping the stream before completion requests termination of the child.
pub type ListerStream = JsonlStream;

/// A listing stream, or a capability-validation or process-start error.
pub type ListerResult = Result<ListerStream, ExecutorError>;

/// An external [lister] that iterates a directory URL and emits RDF for its entries.
///
/// The input URL is passed as one command-line argument, and stdin is connected
/// to the null device. Sorting, numeric offset, and URI cursor bounds are delegated
/// to the external program. `options.limit` is always enforced locally as a
/// maximum number of stdout lines in every output mode; `--limit` is also passed
/// unless native limit support is explicitly unsupported. The local cap works
/// without native support and protects against bugs in programs that accept the
/// flag. Captured stdout is streamed one line at a time with
/// backpressure rather than buffered until the program exits. Stderr is drained
/// concurrently while reading and retained for failure diagnostics.
///
/// Cursor bounds use entry URIs (JSON-LD `@id`) and are exclusive in the selected
/// sort order. Both bounds may define an interval; they cannot be combined with
/// numeric offset. The runner validates URI syntax but does not resolve IDs or
/// inspect graphs; the child defines stable ordering and missing-ID behavior.
///
/// [lister]: https://asimov-specs.github.io/program-patterns/#lister
#[allow(unused)]
#[derive(Debug)]
pub struct Lister {
    executor: Executor,
    options: ListerOptions,
    capabilities: ListerCapabilities,
    input: String,
    output: GraphOutput,
}

impl Lister {
    /// Configures a lister for the directory URL `input` without starting it.
    ///
    /// Adds configured `--sort`, `--offset`, `--before`, `--after`, `--limit`, and
    /// `--output` options as `--name=value` arguments, in that order, followed by
    /// `options.other` and the unvalidated URL. `output` selects stdout handling;
    /// stderr is captured for failure diagnostics. The runner also enforces the
    /// limit locally, even if the child accepts the flag but fails to honor it.
    ///
    /// Native sorting and pagination support defaults to unknown;
    /// use [`with_capabilities`](Self::with_capabilities) to supply known support.
    /// This wrapper enforces limits locally but does not discover capabilities
    /// or emulate sorting, offset, or URI bounds. Unknown or supported options
    /// are forwarded. An unsupported limit flag is omitted; other explicitly
    /// unsupported requests are rejected by [`execute`](Self::execute). Sorting
    /// precedes offset or cursor bounds, and limit applies last. The program
    /// contract counts complete entries, but this wrapper caps serialized lines without parsing
    /// entry boundaries. The SDK formats sort keys using `SortKeys`; the
    /// resulting expression must be supported by the selected program's profile.
    pub fn new(
        program: impl AsRef<OsStr>,
        input: impl AsRef<str>,
        output: GraphOutput,
        options: ListerOptions,
    ) -> Self {
        Self::configured(
            program,
            input,
            output,
            options,
            ListerCapabilities::default(),
        )
    }

    fn configured(
        program: impl AsRef<OsStr>,
        input: impl AsRef<str>,
        output: GraphOutput,
        options: ListerOptions,
        capabilities: ListerCapabilities,
    ) -> Self {
        let input = input.as_ref().to_string();
        let mut executor = Executor::new(program);
        executor
            .command()
            .option("sort", options.sort.as_ref())
            .option("offset", options.offset)
            .option("before", options.before.as_ref())
            .option("after", options.after.as_ref())
            .option(
                "limit",
                options
                    .limit
                    .filter(|_| capabilities.limit != OptionSupport::Unsupported),
            )
            .option("output", options.output.as_ref())
            .args(&options.other)
            .arg(&input)
            .stdin(Stdio::null())
            .stdout(if options.limit.is_some() {
                Stdio::piped()
            } else {
                output.as_stdio()
            })
            .stderr(Stdio::piped());

        Self {
            executor,
            options,
            capabilities,
            input,
            output,
        }
    }

    /// Supplies native option support for the configured program without spawning it.
    ///
    /// Unknown support preserves the default forwarding behavior. Explicitly
    /// supported requests are also forwarded. An unsupported `--limit` is omitted
    /// while retaining the local cap. Other explicitly unsupported requests
    /// fail at execution, without spawning or consuming an output writer. Unset
    /// requests are unaffected by capability metadata. Validation covers the
    /// typed `sort`, `offset`, `before`, and `after` fields, including `Some(0)`
    /// for offset; `other` remains a verbatim argument list and is not parsed.
    ///
    /// The caller may derive this metadata from module manifests; the runner
    /// does not load or verify it. There is no sort/offset/cursor emulation yet.
    /// Future fallbacks must preserve sort → offset or cursor bounds → limit
    /// order: local sorting cannot operate on output truncated by a native limit.
    ///
    /// ```no_run
    /// use asimov_runner::{GraphOutput, Lister, ListerCapabilities, ListerOptions, OptionSupport};
    /// use futures_lite::StreamExt;
    ///
    /// # async fn example() -> Result<(), asimov_runner::ExecutorError> {
    /// let mut lister = Lister::new(
    ///     "asimov-example-lister",
    ///     "https://example.com/collection",
    ///     GraphOutput::Captured,
    ///     ListerOptions::builder().offset(10).limit(25).build(),
    /// ).with_capabilities(ListerCapabilities::builder()
    ///     .sort(OptionSupport::Unsupported)
    ///     .offset(OptionSupport::Supported)
    ///     .limit(OptionSupport::Unsupported)
    ///     .build());
    /// let mut lines = lister.execute().await?;
    /// while let Some(line) = lines.next().await {
    ///     let bytes = line?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: ListerCapabilities) -> Self {
        let program = self
            .executor
            .command()
            .as_std()
            .get_program()
            .to_os_string();
        Self::configured(program, self.input, self.output, self.options, capabilities)
    }

    /// Starts a new lister process and returns its live listing stream, or returns
    /// an empty stream without spawning when the limit is zero.
    ///
    /// Returns after spawning, without waiting for output or process completion.
    /// With captured stdout, each item contains one line, retaining its terminator
    /// when present. A final unterminated line is also yielded.
    /// Other output modes yield no payload lines. With a limit, inherited and
    /// forwarded stdout is routed through the same line cap; ignored stdout is
    /// read and discarded until EOF or the cap.
    /// Stderr is buffered without a size bound; stdout buffers only the current
    /// line and a fixed-size read buffer. JSONL framing does not establish how
    /// many lines or RDF statements belong to one logical listing entry.
    ///
    /// After option and capability validation, `None` leaves output unlimited.
    /// `Some(0)` returns an empty stream without spawning or consuming a writer.
    /// With a positive limit, the child is dropped as soon as the last permitted
    /// line is read, even if the caller
    /// retains the stream. Reaching the cap ends the listing intentionally: no
    /// later output or exit error is observed. Forwarded output is flushed on
    /// completion. Blank and unterminated final lines each count as one line.
    ///
    /// # Errors
    ///
    /// Returns an I/O `InvalidInput` error wrapped in [`ExecutorError::UnexpectedOther`]
    /// for mixed offset/cursor pagination or malformed absolute cursor URIs.
    /// Returns [`ExecutorError::UnsupportedOption`] before spawning if requested
    /// sorting, offset, or a cursor bound is explicitly unsupported, even with a
    /// zero limit. Otherwise returns an [`ExecutorError`] if spawning fails. Subsequent I/O and exit
    /// errors are delivered through the stream, after any preceding output lines.
    pub async fn execute(&mut self) -> ListerResult {
        self.validate_pagination()?;
        for (option, requested, support) in [
            (
                "--sort",
                self.options.sort.is_some(),
                self.capabilities.sort,
            ),
            (
                "--offset",
                self.options.offset.is_some(),
                self.capabilities.offset,
            ),
            (
                "--before",
                self.options.before.is_some(),
                self.capabilities.before,
            ),
            (
                "--after",
                self.options.after.is_some(),
                self.capabilities.after,
            ),
        ] {
            if requested && support == OptionSupport::Unsupported {
                return Err(ExecutorError::UnsupportedOption(option));
            }
        }
        let Some(limit) = self.options.limit else {
            return self
                .executor
                .execute_jsonl_with_output(&mut self.output)
                .await;
        };
        if limit == 0 {
            return Ok(Box::pin(futures_lite::stream::empty()));
        }

        let mut source = self.executor.execute_jsonl().await?;
        let stream: JsonlStream = Box::pin(async_stream::try_stream! {
            for remaining in (0..limit).rev() {
                let Some(line) = source.next().await else {
                    break;
                };
                let line = line?;
                if remaining == 0 {
                    // Drop before yielding so retaining the capped stream cannot
                    // keep an unbounded producer running in the background.
                    drop(source);
                    yield line;
                    return;
                }
                yield line;
            }
        });

        let writer: Option<Box<dyn AsyncWrite + Send + Sync + Unpin>> =
            match self.output.take_for_stream() {
                GraphOutput::Captured => return Ok(stream),
                GraphOutput::Ignored => None,
                GraphOutput::Inherited => Some(Box::new(tokio::io::stdout())),
                GraphOutput::AsyncWrite(writer) => Some(writer),
            };
        Ok(forward_lines(stream, writer))
    }

    fn validate_pagination(&self) -> Result<(), ExecutorError> {
        use std::io::{Error, ErrorKind};
        if self.options.offset.is_some()
            && (self.options.before.is_some() || self.options.after.is_some())
        {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "lister offset cannot be combined with before/after cursors",
            )
            .into());
        }
        for (option, value) in [
            ("--before", self.options.before.as_deref()),
            ("--after", self.options.after.as_deref()),
        ] {
            if let Some(value) = value {
                if value.is_empty()
                    || value.chars().any(|c| c.is_whitespace() || c.is_control())
                    || url::Url::parse(value).is_err()
                {
                    return Err(Error::new(
                        ErrorKind::InvalidInput,
                        format!("lister {option} must be an absolute entry URI (JSON-LD @id)"),
                    )
                    .into());
                }
            }
        }
        Ok(())
    }
}

/// Consumes a bounded listing without returning payload lines to the caller.
fn forward_lines(
    mut stream: JsonlStream,
    mut writer: Option<Box<dyn AsyncWrite + Send + Sync + Unpin>>,
) -> JsonlStream {
    Box::pin(async_stream::stream! {
        let result = async move {
            while let Some(line) = stream.next().await {
                let line = line?;
                if let Some(writer) = &mut writer {
                    writer.write_all(&line).await?;
                }
            }
            if let Some(writer) = &mut writer {
                writer.flush().await?;
            }
            Ok::<(), ExecutorError>(())
        }.await;
        if let Err(error) = result {
            yield Err(error);
        }
    })
}

impl asimov_patterns::Lister<ListerStream> for Lister {}

#[async_trait]
impl asimov_patterns::Execute<ListerStream> for Lister {
    type Error = ExecutorError;

    async fn execute(&mut self) -> ListerResult {
        self.execute().await
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use alloc::vec;
    use futures_lite::StreamExt;
    use std::time::Duration;
    use tokio::time::timeout;

    fn cursor_lister(options: ListerOptions) -> Lister {
        Lister::new(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/lister-cursors.sh"
            ),
            "example:collection",
            GraphOutput::Captured,
            options,
        )
        .with_capabilities(
            ListerCapabilities::builder()
                .sort(OptionSupport::Supported)
                .offset(OptionSupport::Supported)
                .before(OptionSupport::Supported)
                .after(OptionSupport::Supported)
                .limit(OptionSupport::Unsupported)
                .build(),
        )
    }

    #[test]
    fn limit_forwarding_tracks_capability_changes_without_stale_arguments() {
        let mut lister = Lister::new(
            "asimov-test-lister",
            "example:",
            GraphOutput::Captured,
            ListerOptions::builder().offset(2).limit(3).build(),
        );
        for support in [
            OptionSupport::Unknown,
            OptionSupport::Unsupported,
            OptionSupport::Supported,
            OptionSupport::Unsupported,
            OptionSupport::Unknown,
        ] {
            lister = lister.with_capabilities(ListerCapabilities::builder().limit(support).build());
            let arguments: alloc::vec::Vec<_> =
                lister.executor.command().as_std().get_args().collect();
            if support == OptionSupport::Unsupported {
                assert_eq!(arguments, ["--offset=2", "example:"]);
            } else {
                assert_eq!(arguments, ["--offset=2", "--limit=3", "example:"]);
            }
        }
    }

    #[tokio::test]
    async fn cursor_bounds_follow_rank_order_and_preserve_numeric_pagination() {
        let cases = [
            (
                ListerOptions::builder().offset(1).limit(2).build(),
                vec!["urn:item:zeta", "urn:item:beta"],
            ),
            (
                ListerOptions::builder()
                    .after("urn:item:zeta")
                    .limit(1)
                    .build(),
                vec!["urn:item:beta"],
            ),
            (
                ListerOptions::builder()
                    .before("urn:item:beta")
                    .limit(2)
                    .build(),
                vec!["urn:item:alpha", "urn:item:zeta"],
            ),
            (
                ListerOptions::builder()
                    .after("urn:item:alpha")
                    .before("urn:item:omega")
                    .limit(5)
                    .build(),
                vec!["urn:item:zeta", "urn:item:beta"],
            ),
            (
                ListerOptions::builder()
                    .sort("-rank".parse().unwrap())
                    .after("urn:item:beta")
                    .limit(1)
                    .build(),
                vec!["urn:item:zeta"],
            ),
            (
                ListerOptions::builder()
                    .before("urn:item:alpha")
                    .limit(1)
                    .build(),
                vec![],
            ),
        ];
        for (options, expected) in cases {
            let mut stream = cursor_lister(options).execute().await.unwrap();
            for id in expected {
                assert_eq!(
                    stream.next().await.unwrap().unwrap(),
                    format!("{{\"@id\":\"{id}\"}}\n").as_bytes()
                );
            }
            assert!(stream.next().await.is_none());
        }
    }

    #[tokio::test]
    async fn uri_cursor_is_stable_when_an_earlier_entry_is_inserted() {
        for other in [vec![], vec!["--inserted".into()]] {
            let mut stream = cursor_lister(ListerOptions {
                after: Some("urn:item:zeta".into()),
                limit: Some(1),
                other,
                ..Default::default()
            })
            .execute()
            .await
            .unwrap();
            assert_eq!(
                stream.next().await.unwrap().unwrap(),
                b"{\"@id\":\"urn:item:beta\"}\n"
            );
            assert!(stream.next().await.is_none());
        }
    }

    #[tokio::test]
    async fn cursor_arguments_preserve_uri_spelling_and_boundaries() {
        for support in [OptionSupport::Unknown, OptionSupport::Supported] {
            let mut lister = Lister::new(
                "/this-lister-does-not-exist",
                "example:",
                GraphOutput::Captured,
                ListerOptions::builder()
                    .sort("rank".parse().unwrap())
                    .before("HTTPS://Example.COM/a%2fb?x=a=b&y=c#end")
                    .after("urn:example:item:123")
                    .limit(3)
                    .output("jsonl")
                    .build(),
            )
            .with_capabilities(
                ListerCapabilities::builder()
                    .before(support)
                    .after(support)
                    .build(),
            );
            let arguments: alloc::vec::Vec<_> =
                lister.executor.command().as_std().get_args().collect();
            assert_eq!(
                arguments,
                [
                    "--sort=rank",
                    "--before=HTTPS://Example.COM/a%2fb?x=a=b&y=c#end",
                    "--after=urn:example:item:123",
                    "--limit=3",
                    "--output=jsonl",
                    "example:"
                ]
            );
            assert!(matches!(
                lister.execute().await,
                Err(ExecutorError::MissingProgram(_))
            ));
        }
    }

    #[tokio::test]
    async fn rejects_invalid_cursor_pagination_before_spawn() {
        for value in [
            "",
            "relative/path",
            "_:blank",
            "not a URI",
            "urn:example:a\n",
            " urn:example:a",
        ] {
            for before in [true, false] {
                let options = if before {
                    ListerOptions::builder().before(value).limit(0).build()
                } else {
                    ListerOptions::builder().after(value).limit(0).build()
                };
                let mut lister = Lister::new(
                    "/this-lister-does-not-exist",
                    "example:",
                    GraphOutput::AsyncWrite(Box::new(tokio::io::sink())),
                    options,
                );
                assert!(
                    matches!(lister.execute().await, Err(ExecutorError::UnexpectedOther(error))
                    if error.kind() == std::io::ErrorKind::InvalidInput)
                );
                assert!(matches!(lister.output, GraphOutput::AsyncWrite(_)));
            }
        }
        for offset in [0, 2] {
            for before in [true, false] {
                let options = if before {
                    ListerOptions::builder()
                        .before("urn:item:beta")
                        .offset(offset)
                        .build()
                } else {
                    ListerOptions::builder()
                        .after("urn:item:beta")
                        .offset(offset)
                        .build()
                };
                let mut lister = Lister::new(
                    "/this-lister-does-not-exist",
                    "example:",
                    GraphOutput::Captured,
                    options,
                );
                assert!(
                    matches!(lister.execute().await, Err(ExecutorError::UnexpectedOther(error))
                    if error.kind() == std::io::ErrorKind::InvalidInput)
                );
            }
        }
    }

    #[tokio::test]
    async fn rejects_unsupported_cursor_bounds_but_allows_unsupported_limit() {
        for (options, option) in [
            (
                ListerOptions::builder()
                    .before("urn:item:beta")
                    .limit(1)
                    .build(),
                "--before",
            ),
            (
                ListerOptions::builder()
                    .after("urn:item:beta")
                    .limit(1)
                    .build(),
                "--after",
            ),
        ] {
            let mut lister = Lister::new(
                "/this-lister-does-not-exist",
                "example:",
                GraphOutput::Captured,
                options,
            )
            .with_capabilities(
                ListerCapabilities::builder()
                    .before(OptionSupport::Unsupported)
                    .after(OptionSupport::Unsupported)
                    .limit(OptionSupport::Unsupported)
                    .build(),
            );
            assert!(
                matches!(lister.execute().await, Err(ExecutorError::UnsupportedOption(actual)) if actual == option)
            );
        }
        let mut lister = Lister::new(
            "/this-lister-does-not-exist",
            "example:",
            GraphOutput::Captured,
            ListerOptions::builder().limit(0).build(),
        )
        .with_capabilities(
            ListerCapabilities::builder()
                .limit(OptionSupport::Unsupported)
                .build(),
        );
        assert!(lister.execute().await.unwrap().next().await.is_none());
    }

    #[tokio::test]
    async fn unsupported_native_limit_still_terminates_a_long_running_child() {
        for output in [GraphOutput::Captured, GraphOutput::Ignored] {
            let captured = matches!(output, GraphOutput::Captured);
            let mut stream = Lister::new(
                "/bin/sh",
                "printf '{}\\n[]\\n'; exec sleep 30",
                output,
                ListerOptions::builder().limit(1).other("-c").build(),
            )
            .with_capabilities(
                ListerCapabilities::builder()
                    .limit(OptionSupport::Unsupported)
                    .build(),
            )
            .execute()
            .await
            .unwrap();
            timeout(Duration::from_secs(5), async {
                if captured {
                    assert_eq!(stream.next().await.unwrap().unwrap(), b"{}\n");
                }
                assert!(stream.next().await.is_none());
            })
            .await
            .expect("local limit must work without native --limit");
        }
    }

    #[tokio::test]
    async fn rejects_explicitly_unsupported_requests_before_spawning() {
        for limit in [None, Some(0), Some(1)] {
            for (options, option) in [
                (
                    ListerOptions {
                        sort: Some("name".parse().unwrap()),
                        limit,
                        ..Default::default()
                    },
                    "--sort",
                ),
                (
                    ListerOptions {
                        offset: Some(0),
                        limit,
                        ..Default::default()
                    },
                    "--offset",
                ),
                (
                    ListerOptions {
                        offset: Some(10),
                        limit,
                        ..Default::default()
                    },
                    "--offset",
                ),
            ] {
                let mut lister = Lister::new(
                    "/this-lister-does-not-exist",
                    "example:",
                    GraphOutput::AsyncWrite(Box::new(tokio::io::sink())),
                    options,
                )
                .with_capabilities(ListerCapabilities {
                    sort: OptionSupport::Unsupported,
                    offset: OptionSupport::Unsupported,
                    ..Default::default()
                });
                assert!(matches!(lister.execute().await,
                    Err(ExecutorError::UnsupportedOption(actual)) if actual == option));
                assert!(matches!(lister.output, GraphOutput::AsyncWrite(_)));
            }
        }
    }

    #[tokio::test]
    async fn supported_and_unknown_requests_are_forwarded() {
        for sort in [OptionSupport::Unknown, OptionSupport::Supported] {
            for offset in [OptionSupport::Unknown, OptionSupport::Supported] {
                let mut lister = Lister::new(
                    "/this-lister-does-not-exist",
                    "example:",
                    GraphOutput::Captured,
                    ListerOptions::builder()
                        .sort("name".parse().unwrap())
                        .offset(2)
                        .limit(3)
                        .build(),
                )
                .with_capabilities(ListerCapabilities {
                    sort,
                    offset,
                    ..Default::default()
                });
                let arguments: alloc::vec::Vec<_> =
                    lister.executor.command().as_std().get_args().collect();
                assert_eq!(
                    arguments,
                    ["--sort=name", "--offset=2", "--limit=3", "example:"]
                );
                // Reaching spawn rather than capability rejection proves both
                // Unknown and Supported retain the configured requests.
                assert!(matches!(
                    lister.execute().await,
                    Err(ExecutorError::MissingProgram(_))
                ));
            }
        }
    }

    #[tokio::test]
    async fn unsupported_unrequested_options_do_not_disable_limit_enforcement() {
        let mut lister = limited_shell(
            "printf '{}\\n[]\\n'; exec sleep 30",
            GraphOutput::Captured,
            Some(1),
        )
        .with_capabilities(ListerCapabilities {
            sort: OptionSupport::Unsupported,
            offset: OptionSupport::Unsupported,
            ..Default::default()
        });
        let mut stream = lister.execute().await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(5), stream.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            b"{}\n"
        );
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn capability_rejection_does_not_poison_later_execution() {
        let lister = Lister::new(
            "/this-lister-does-not-exist",
            "example:",
            GraphOutput::Captured,
            ListerOptions::builder().offset(1).build(),
        );
        assert_eq!(lister.capabilities, ListerCapabilities::default());
        let mut lister = lister.with_capabilities(
            ListerCapabilities::builder()
                .offset(OptionSupport::Unsupported)
                .build(),
        );
        assert!(matches!(
            lister.execute().await,
            Err(ExecutorError::UnsupportedOption("--offset"))
        ));
        let mut lister = lister.with_capabilities(
            ListerCapabilities::builder()
                .offset(OptionSupport::Supported)
                .build(),
        );
        assert!(matches!(
            lister.execute().await,
            Err(ExecutorError::MissingProgram(_))
        ));
    }

    fn shell(script: &str, output: GraphOutput) -> Lister {
        limited_shell(script, output, None)
    }

    fn limited_shell(script: &str, output: GraphOutput, limit: Option<usize>) -> Lister {
        let program = if limit.is_some() {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/lister-ignores-limit.sh"
            )
        } else {
            "/bin/sh"
        };
        Lister::new(
            program,
            script,
            output,
            ListerOptions {
                limit,
                other: vec!["-c".into()],
                ..Default::default()
            },
        )
    }

    #[test]
    fn forwards_limit_between_offset_and_output() {
        let mut lister = Lister::new(
            "asimov-test-lister",
            "example:",
            GraphOutput::Captured,
            ListerOptions::builder()
                .offset(2)
                .limit(3)
                .output("jsonl")
                .other("--custom")
                .build(),
        );
        let arguments: alloc::vec::Vec<_> = lister.executor.command().as_std().get_args().collect();
        assert_eq!(
            arguments,
            [
                "--offset=2",
                "--limit=3",
                "--output=jsonl",
                "--custom",
                "example:"
            ]
        );

        for (limit, expected) in [(None, None), (Some(0), Some("--limit=0"))] {
            let mut lister = limited_shell("exit 0", GraphOutput::Captured, limit);
            let arguments: alloc::vec::Vec<_> =
                lister.executor.command().as_std().get_args().collect();
            assert_eq!(arguments.len(), if limit.is_some() { 3 } else { 2 });
            if let Some(expected) = expected {
                assert_eq!(arguments[0], expected);
            }
        }
    }

    #[tokio::test]
    async fn enforces_line_limits_when_program_ignores_the_flag() {
        timeout(Duration::from_secs(5), async {
            let expected = [b"{}\n".as_slice(), b"\r\n", b"\xff\n", b"{\"last\":true}"];
            for limit in [0, 1, 2, 3, 4, 5, usize::MAX] {
                // The fixture requires --limit but emits all four lines anyway.
                let mut stream = limited_shell(
                    "printf '{}\\n\\r\\n\\377\\n{\"last\":true}'",
                    GraphOutput::Captured,
                    Some(limit),
                )
                .execute()
                .await
                .unwrap();
                for line in &expected[..limit.min(expected.len())] {
                    assert_eq!(stream.next().await.unwrap().unwrap(), *line);
                }
                assert!(stream.next().await.is_none());
                assert!(stream.next().await.is_none());
            }
        })
        .await
        .expect("a bounded listing must complete promptly");
    }

    #[tokio::test]
    async fn zero_limit_does_not_spawn() {
        for output in [
            GraphOutput::Captured,
            GraphOutput::Ignored,
            GraphOutput::Inherited,
        ] {
            let mut stream = Lister::new(
                "/this-lister-does-not-exist",
                "example:",
                output,
                ListerOptions::builder().limit(0).build(),
            )
            .execute()
            .await
            .unwrap();
            assert!(stream.next().await.is_none());
        }
    }

    #[tokio::test]
    async fn reaching_limit_terminates_child_before_stream_is_dropped() {
        let mut stream = limited_shell(
            "printf '%s\\n' $$; exec sleep 30",
            GraphOutput::Captured,
            Some(1),
        )
        .execute()
        .await
        .unwrap();
        let pid = timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let pid = std::str::from_utf8(&pid).unwrap().trim();
        // Keep the stream alive, without polling it again, while checking the child.
        timeout(Duration::from_secs(5), async {
            while tokio::process::Command::new("/bin/kill")
                .args(["-0", pid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .unwrap()
                .success()
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the line cap must terminate the child immediately");
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn limits_ignored_output_without_waiting_for_exit() {
        let mut stream = limited_shell(
            "printf '{}\\n[]\\n{\"extra\":true}\\n'; exec sleep 30",
            GraphOutput::Ignored,
            Some(2),
        )
        .execute()
        .await
        .unwrap();
        assert!(
            timeout(Duration::from_secs(5), stream.next())
                .await
                .expect("ignored output must also observe the cap")
                .is_none()
        );
    }

    #[tokio::test]
    async fn reports_failure_before_limit_but_not_after_it() {
        for limit in [1, 2] {
            let mut stream = limited_shell(
                "printf '{}\\n'; printf 'failed' >&2; exit 1",
                GraphOutput::Captured,
                Some(limit),
            )
            .execute()
            .await
            .unwrap();
            assert_eq!(stream.next().await.unwrap().unwrap(), b"{}\n");
            if limit == 2 {
                assert!(matches!(
                    stream.next().await,
                    Some(Err(ExecutorError::UnexpectedFailure(Some(1), Some(stderr))))
                        if stderr == "failed"
                ));
            }
            assert!(stream.next().await.is_none());
        }
    }

    #[tokio::test]
    async fn streams_before_process_exit() {
        let mut stream = timeout(
            Duration::from_secs(5),
            shell("printf '{}\\n'; exec sleep 30", GraphOutput::Captured).execute(),
        )
        .await
        .expect("execute must return before the process exits")
        .unwrap();
        let line = timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("the first line must arrive before the process exits");
        assert_eq!(line.unwrap().unwrap(), b"{}\n");
        // Dropping the stream terminates the still-running child.
    }

    #[tokio::test]
    async fn preserves_line_bytes_and_unterminated_tail() {
        let mut stream = shell(
            "printf '{}\\n\\r\\n\\377\\n{\"last\":true}'",
            GraphOutput::Captured,
        )
        .execute()
        .await
        .unwrap();
        for expected in [b"{}\n".as_slice(), b"\r\n", b"\xff\n", b"{\"last\":true}"] {
            assert_eq!(stream.next().await.unwrap().unwrap(), expected);
        }
        assert!(stream.next().await.is_none());
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn drains_stderr_and_reports_failure_after_lines() {
        timeout(Duration::from_secs(5), async {
            let mut stream = shell(
                "i=0; while [ $i -lt 10000 ]; do printf 'diagnostic\\n' >&2; i=$((i + 1)); done; printf '{}\\n'; exit 1",
                GraphOutput::Captured,
            )
            .execute()
            .await
            .unwrap();
            assert_eq!(stream.next().await.unwrap().unwrap(), b"{}\n");
            match stream.next().await.unwrap().unwrap_err() {
                ExecutorError::UnexpectedFailure(Some(1), Some(stderr)) => {
                    assert_eq!(stderr, "diagnostic\n".repeat(10000));
                },
                error => panic!("unexpected error: {error}"),
            }
            assert!(stream.next().await.is_none());
        })
        .await
        .expect("stderr must be drained concurrently to avoid deadlock");
    }

    #[tokio::test]
    async fn uncaptured_stdout_still_checks_exit_status() {
        let mut stream = shell("printf '{}\\n'", GraphOutput::Ignored)
            .execute()
            .await
            .unwrap();
        assert!(stream.next().await.is_none());

        let mut stream = shell("exit 1", GraphOutput::Ignored)
            .execute()
            .await
            .unwrap();
        assert!(matches!(
            stream.next().await,
            Some(Err(ExecutorError::UnexpectedFailure(Some(1), _)))
        ));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn reports_spawn_errors_from_execute() {
        let result = Lister::new(
            "/this-lister-does-not-exist",
            "example:",
            GraphOutput::Captured,
            Default::default(),
        )
        .execute()
        .await;
        assert!(matches!(result, Err(ExecutorError::MissingProgram(_))));
    }
}
