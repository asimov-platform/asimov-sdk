// This is free and unencumbered software released into the public domain.

use crate::*;
use alloc::{boxed::Box, string::ToString, vec, vec::Vec};
use core::{
    pin::Pin,
    task::{Context, Poll},
};
use futures_lite::StreamExt;
use std::{
    io::{self, Cursor},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{io::AsyncWrite, time::timeout};

#[derive(Default)]
struct Written {
    bytes: Vec<u8>,
    flushes: usize,
    shutdowns: usize,
}

struct Writer {
    state: Arc<Mutex<Written>>,
    fail_write: bool,
    fail_flush: bool,
}

impl AsyncWrite for Writer {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.fail_write {
            return Poll::Ready(Err(io::Error::other("destination write failed")));
        }
        // Partial writes exercise forwarding's write-all behavior.
        let count = bytes.len().min(3);
        self.state
            .lock()
            .unwrap()
            .bytes
            .extend_from_slice(&bytes[..count]);
        Poll::Ready(Ok(count))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.state.lock().unwrap().flushes += 1;
        Poll::Ready(if self.fail_flush {
            Err(io::Error::other("destination flush failed"))
        } else {
            Ok(())
        })
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.state.lock().unwrap().shutdowns += 1;
        Poll::Ready(Ok(()))
    }
}

fn destination() -> (Output, Arc<Mutex<Written>>) {
    let state = Arc::new(Mutex::new(Written::default()));
    (
        Output::AsyncWrite(Box::new(Writer {
            state: state.clone(),
            fail_write: false,
            fail_flush: false,
        })),
        state,
    )
}

fn shell(script: &str, input: &Input, output: &Output) -> Executor {
    let mut executor = Executor::new("/bin/sh");
    executor
        .command()
        .args(["-c", script])
        .stdin(input.as_stdio())
        .stdout(output.as_stdio())
        .stderr(Stdio::piped());
    executor
}

#[tokio::test]
async fn lister_caps_forwarded_output_and_flushes() {
    for support in [
        OptionSupport::Unknown,
        OptionSupport::Supported,
        OptionSupport::Unsupported,
    ] {
        let (output, state) = destination();
        let program = if support == OptionSupport::Unsupported {
            "/bin/sh"
        } else {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/lister-ignores-limit.sh"
            )
        };
        let mut stream = Lister::new(
            program,
            "printf '{}\\r\\n[]\\n{\"extra\":true}\\n'; exec sleep 30",
            output,
            ListerOptions::builder().limit(2).other("-c").build(),
        )
        .with_capabilities(ListerCapabilities::builder().limit(support).build())
        .execute()
        .await
        .unwrap();
        assert!(
            timeout(Duration::from_secs(5), stream.next())
                .await
                .expect("forwarding must finish at the line cap")
                .is_none()
        );
        let written = state.lock().unwrap();
        assert_eq!(written.bytes, b"{}\r\n[]\n");
        assert!(written.flushes >= 1);
        assert_eq!(written.shutdowns, 0);
    }
}

#[tokio::test]
async fn capped_lister_propagates_writer_failures() {
    for (fail_write, fail_flush) in [(true, false), (false, true)] {
        let output = Output::AsyncWrite(Box::new(Writer {
            state: Arc::default(),
            fail_write,
            fail_flush,
        }));
        let mut stream = Lister::new(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/lister-ignores-limit.sh"
            ),
            "printf '{}\\n'; exec sleep 30",
            output,
            ListerOptions::builder().limit(1).other("-c").build(),
        )
        .execute()
        .await
        .unwrap();
        let result = timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(result, Err(ExecutorError::UnexpectedOther(error))
            if error.to_string().contains(if fail_write { "write failed" } else { "flush failed" })));
        assert!(stream.next().await.is_none());
    }
}

#[tokio::test]
async fn buffered_forwarding_is_incremental_and_writer_is_reusable() {
    let (mut output, state) = destination();
    let mut input = Input::Ignored;
    let mut executor = shell("printf 'abcdef\\000\\377'", &input, &output);
    for _ in 0..2 {
        assert!(
            executor
                .execute_with_io(&mut input, &mut output)
                .await
                .unwrap()
                .into_inner()
                .is_empty()
        );
    }
    let written = state.lock().unwrap();
    assert_eq!(written.bytes, b"abcdef\x00\xffabcdef\x00\xff");
    assert!(written.flushes >= 2);
    assert_eq!(written.shutdowns, 0);
}

#[tokio::test]
async fn graph_forwarding_moves_writer_and_reports_exit_errors() {
    let (output, state) = destination();
    let mut emitter = Emitter::new(
        "/bin/sh",
        output,
        EmitterOptions::builder()
            .other("-c")
            .other("printf '{}\\n'; printf 'bad graph' >&2; exit 65")
            .build(),
    );
    let mut stream = emitter.execute().await.unwrap();
    assert!(
        matches!(stream.next().await, Some(Err(ExecutorError::Failure(_, Some(stderr)))) if stderr == "bad graph")
    );
    assert!(stream.next().await.is_none());
    assert_eq!(state.lock().unwrap().bytes, b"{}\n");
    assert!(state.lock().unwrap().flushes >= 1);

    // The consumed writer is not silently reused or replaced by capture.
    let mut stream = emitter.execute().await.unwrap();
    assert!(stream.next().await.unwrap().is_err());
    assert!(stream.next().await.is_none());
    assert_eq!(state.lock().unwrap().bytes, b"{}\n");
}

#[tokio::test]
async fn forwarding_write_and_flush_failures_propagate() {
    for (fail_write, fail_flush) in [(true, false), (false, true)] {
        let mut output = Output::AsyncWrite(Box::new(Writer {
            state: Arc::default(),
            fail_write,
            fail_flush,
        }));
        let mut input = Input::Ignored;
        let script = if fail_write {
            "printf '{}\\n'; exec sleep 30"
        } else {
            "printf '{}\\n'"
        };
        let mut executor = shell(script, &input, &output);
        let mut stream = executor
            .execute_jsonl_with_io(&mut input, &mut output)
            .await
            .unwrap();
        let result = timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(result, Err(ExecutorError::UnexpectedOther(error))
            if error.to_string().contains(if fail_write { "write failed" } else { "flush failed" })));
        assert!(stream.next().await.is_none());
    }
}

#[tokio::test]
async fn prompter_honors_output_modes_and_can_be_reused() {
    let prompt = Prompt::builder()
        .messages(vec![PromptMessage(PromptRole::User, "hello".into())])
        .build();
    for (output, script) in [
        (Output::Ignored, "cat"),
        (Output::Inherited, "cat >/dev/null"),
    ] {
        let mut prompter = Prompter::new(
            "/bin/sh",
            prompt.clone(),
            output,
            PrompterOptions::builder().other("-c").other(script).build(),
        );
        assert!(prompter.execute().await.unwrap().is_empty());
    }
    let (output, state) = destination();
    let mut prompter = Prompter::new("/bin/cat", prompt.clone(), output, Default::default());
    for _ in 0..2 {
        assert!(prompter.execute().await.unwrap().is_empty());
    }
    assert_eq!(state.lock().unwrap().bytes, b"user: hello\nuser: hello\n");

    let mut prompter = Prompter::new(
        "/bin/sh",
        prompt,
        Output::Captured,
        PrompterOptions::builder()
            .other("-c")
            .other("cat >/dev/null; printf '\\377'")
            .build(),
    );
    assert!(
        matches!(prompter.execute().await, Err(ExecutorError::UnexpectedOther(error))
        if error.kind() == io::ErrorKind::InvalidData)
    );
}

#[tokio::test]
async fn forwarded_bytes_arrive_before_process_exit() {
    let (output, state) = destination();
    let mut stream = Emitter::new(
        "/bin/sh",
        output,
        EmitterOptions::builder()
            .other("-c")
            .other("printf '{}\\n'; exec sleep 30")
            .build(),
    )
    .execute()
    .await
    .unwrap();
    timeout(Duration::from_secs(5), async {
        tokio::select! {
            result = stream.next() => panic!("process unexpectedly completed: {result:?}"),
            _ = async {
                while state.lock().unwrap().bytes != b"{}\n" {
                    tokio::task::yield_now().await;
                }
            } => {},
        }
    })
    .await
    .expect("forwarding must not wait for process exit");
}

#[tokio::test]
async fn prompter_reports_failed_prompt_delivery() {
    let prompt = Prompt::builder()
        .messages(vec![PromptMessage(
            PromptRole::User,
            "x".repeat(1024 * 1024),
        )])
        .build();
    let mut prompter = Prompter::new(
        "/bin/sh",
        prompt,
        Output::Captured,
        PrompterOptions::builder()
            .other("-c")
            .other("printf 'rejected prompt' >&2; exit 65")
            .build(),
    );
    let result = timeout(Duration::from_secs(5), prompter.execute())
        .await
        .unwrap();
    assert!(
        matches!(result, Err(ExecutorError::Failure(_, Some(stderr))) if stderr == "rejected prompt")
    );
}

#[tokio::test]
async fn completion_separates_successful_exit_from_interrupted_input() {
    let mut input = Input::Jsonl(Box::pin(futures_lite::stream::pending()));
    let mut output = Output::Captured;
    let mut executor = shell("printf 'done'", &input, &output);
    let completion = timeout(
        Duration::from_secs(5),
        executor.execute_with_io_completion(&mut input, &mut output),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(completion.output.status.success());
    assert_eq!(completion.output.stdout, b"done");
    assert!(matches!(completion.input, InputCompletion::Interrupted));
    assert!(matches!(
        completion.into_result(),
        Err(ExecutorError::IncompleteInput)
    ));

    let mut stream = executor.execute_jsonl_with_input(&mut input).await.unwrap();
    assert_eq!(stream.next().await.unwrap().unwrap(), b"done");
    assert!(matches!(
        stream.next().await,
        Some(Err(ExecutorError::IncompleteInput))
    ));
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn broken_pipe_does_not_hide_exit_diagnostics() {
    let mut input = Input::AsyncRead(Box::new(Cursor::new(vec![b'x'; 1024 * 1024])));
    let mut output = Output::Captured;
    let mut executor = shell(
        "exec 0<&-; printf 'invalid input' >&2; exit 65",
        &input,
        &output,
    );
    let completion = timeout(
        Duration::from_secs(5),
        executor.execute_with_io_completion(&mut input, &mut output),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(completion.output.status.code(), Some(65));
    assert_eq!(completion.output.stderr, b"invalid input");
    assert!(!matches!(completion.input, InputCompletion::Complete));
    assert!(
        matches!(completion.into_result(), Err(ExecutorError::Failure(_, Some(stderr))) if stderr == "invalid input")
    );
}

#[tokio::test]
async fn upstream_error_terminates_child_and_is_not_masked_by_signal() {
    let source = futures_lite::stream::iter([Err(ExecutorError::UnexpectedOther(
        io::Error::other("source failed"),
    ))]);
    let mut input = Input::Jsonl(Box::pin(source));
    let mut output = Output::Captured;
    let mut executor = shell("exec sleep 30", &input, &output);
    let completion = timeout(
        Duration::from_secs(5),
        executor.execute_with_io_completion(&mut input, &mut output),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(completion.input, InputCompletion::SourceFailed(_)));
    assert!(
        matches!(completion.into_result(), Err(ExecutorError::UnexpectedOther(error)) if error.to_string() == "source failed")
    );
}

#[tokio::test]
async fn resolver_forwards_without_parsing() {
    let (output, state) = destination();
    let mut resolver = Resolver::new(
        "/bin/sh",
        "printf '\\377not a URL'",
        output,
        ResolverOptions::builder().other("-c").build(),
    );
    assert!(resolver.execute().await.unwrap().is_empty());
    assert_eq!(state.lock().unwrap().bytes, b"\xffnot a URL");
}
