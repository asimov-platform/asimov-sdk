// This is free and unencumbered software released into the public domain.

// A custom test harness doubles as a portable subprocess fixture. Unlike a shell
// script or a libtest child, fixture mode emits only the requested payload.
use asimov_runner::*;
use futures_lite::StreamExt;
use std::{
    env,
    io::{self, Cursor, Read, Write},
    path::Path,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWrite};

const RECORD: &[u8] = b"{\"@id\":\"urn:example:record\"}\n";

fn main() {
    let args: Vec<_> = env::args().collect();
    if let Some(position) = args.iter().position(|arg| arg == "--fixture") {
        if let Err(error) = fixture(&args[position + 1], &args) {
            eprintln!("fixture I/O failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(tests());
}

fn fixture(mode: &str, args: &[String]) -> io::Result<()> {
    let value = |prefix: &str| args.iter().find_map(|arg| arg.strip_prefix(prefix));
    let mut notification = value("--notify=")
        .map(std::net::TcpStream::connect)
        .transpose()?;
    if let Some(socket) = &mut notification {
        socket.write_all(&[1])?;
    }
    if let Some(count) = value("--stderr-bytes=") {
        io::stderr().write_all(&vec![b'e'; count.parse::<usize>().unwrap()])?;
    }
    match mode {
        "echo" => {
            let mut input = io::stdin().lock();
            let mut output = io::stdout().lock();
            let mut bytes = [0; 8192];
            loop {
                let count = input.read(&mut bytes)?;
                if count == 0 {
                    break;
                }
                output.write_all(&bytes[..count])?;
                output.flush()?;
            }
        },
        "emit" | "emit-hang" => {
            let count = value("--count=").unwrap_or("1").parse::<usize>().unwrap();
            let mut output = io::stdout().lock();
            for _ in 0..count {
                output.write_all(RECORD)?;
            }
            output.flush()?;
            if mode == "emit-hang" {
                std::thread::sleep(Duration::from_secs(30));
            }
        },
        "hang" => std::thread::sleep(Duration::from_secs(30)),
        "fail" => {
            io::stderr().write_all(b"fixture rejected input")?;
            std::process::exit(65);
        },
        "index" | "binary" => {
            let count = io::copy(&mut io::stdin().lock(), &mut io::sink())?;
            if let Some(expected) = value("--expect-bytes=") {
                assert_eq!(count, expected.parse::<u64>().unwrap());
            }
            if mode == "binary" {
                io::stdout().write_all(b"\0\xffexported\n")?;
            }
        },
        _ => panic!("unknown fixture mode: {mode}"),
    }
    drop(notification);
    Ok(())
}

fn args(mode: &str) -> Vec<String> {
    vec!["--fixture".into(), mode.into()]
}

fn reader(program: &Path, input: Input, mode: &str) -> Reader {
    Reader::new(
        program,
        input,
        Output::Captured,
        ReaderOptions {
            other: args(mode),
            ..Default::default()
        },
    )
}

fn reasoner(program: &Path, mode: &str) -> Reasoner {
    Reasoner::new(
        program,
        Input::Ignored,
        Output::Captured,
        ReasonerOptions {
            other: args(mode),
            ..Default::default()
        },
    )
}

fn writer(program: &Path, mode: &str, output: Output) -> Writer {
    Writer::new(
        program,
        Input::Ignored,
        output,
        WriterOptions {
            other: args(mode),
            ..Default::default()
        },
    )
}

async fn tests() {
    let program = env::current_exe().unwrap();
    let mut passed = 0;
    macro_rules! case {
        ($name:literal, $future:expr) => {{
            tokio::time::timeout(Duration::from_secs(10), $future)
                .await
                .expect(concat!($name, " timed out"));
            println!(concat!($name, " ... ok"));
            passed += 1;
        }};
    }
    case!("reader_writer_large_full_duplex", large_graph(&program));
    case!("fetcher_reasoner_indexer", indexing(&program));
    case!("writer_binary_output", binary_output(&program));
    case!(
        "graph_streaming_and_drop",
        streaming_and_drop(&program, true)
    );
    case!("unpolled_stream_drop", streaming_and_drop(&program, false));
    case!("upstream_failure", stage_failure(&program, false));
    case!(
        "middle_failure_cancels_source",
        stage_failure(&program, true)
    );
    case!("partial_spawn_cleanup", spawn_failure(&program));
    case!("pipeline_preflight", preflight(&program));
    case!("limited_lister", limited_lister(&program));
    case!("external_input_failure", input_failure(&program));
    case!("writer_destination_failure", output_failure(&program));
    case!("forwarded_graph_tail", forwarded_graph(&program));
    case!(
        "successful_forwarding_and_ignored_output",
        output_policies(&program)
    );
    case!(
        "graph_stream_reports_upstream_failure",
        graph_failure(&program)
    );
    case!("single_stage_pipelines", single_stage(&program));
    case!("configured_graph_batches", configured_batches(&program));
    case!("lister_limit_precedes_batching", limited_batches(&program));
    println!("pipeline integration tests: {passed} passed");
}

async fn large_graph(program: &Path) {
    let mut graph = RECORD.repeat(150_000);
    graph.extend_from_slice(b"{\"@id\":\"urn:unterminated\"}");
    let mut source_args = args("echo");
    source_args.push("--stderr-bytes=262144".into());
    let source = Reader::new(
        program,
        Input::AsyncRead(Box::new(Cursor::new(graph.clone()))),
        Output::Captured,
        ReaderOptions {
            other: source_args.clone(),
            ..Default::default()
        },
    );
    let middle = Reasoner::new(
        program,
        Input::Ignored,
        Output::Captured,
        ReasonerOptions {
            other: source_args,
            ..Default::default()
        },
    );
    let result = Pipeline::new(source)
        .pipe(middle)
        .pipe(writer(program, "echo", Output::Captured))
        .execute()
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        result, graph,
        "native edges must preserve every byte, including the final unterminated line"
    );
}

async fn indexing(program: &Path) {
    let mut source_args = args("emit");
    source_args.push("--count=3".into());
    let source = Fetcher::new(
        program,
        "https://example.com/",
        Output::Captured,
        FetcherOptions {
            other: source_args,
            ..Default::default()
        },
    );
    let mut sink_args = args("index");
    sink_args.push(format!("--expect-bytes={}", RECORD.len() * 3));
    let sink = Indexer::new(
        program,
        Input::Ignored,
        IndexerOptions {
            other: sink_args,
            ..Default::default()
        },
    );
    Pipeline::new(source)
        .pipe(reasoner(program, "echo"))
        .pipe(sink)
        .execute()
        .await
        .unwrap();
}

async fn binary_output(program: &Path) {
    let source = reader(
        program,
        Input::AsyncRead(Box::new(Cursor::new(RECORD))),
        "echo",
    );
    let bytes = Pipeline::new(source)
        .pipe(writer(program, "binary", Output::Captured))
        .execute()
        .await
        .unwrap()
        .into_inner();
    assert_eq!(bytes, b"\0\xffexported\n");
}

async fn streaming_and_drop(program: &Path, poll: bool) {
    // TCP EOF provides a portable indication that each child has terminated.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let notify = format!("--notify={}", listener.local_addr().unwrap());
    let mut source_args = args(if poll { "emit-hang" } else { "hang" });
    source_args.push(notify.clone());
    let mut sink_args = args("echo");
    sink_args.push(notify);
    let source = Emitter::new(
        program,
        Output::Captured,
        EmitterOptions {
            other: source_args,
            ..Default::default()
        },
    )
    .with_batching(BatchOptions::new(1000, 1024 * 1024, Duration::from_secs(30)).unwrap());
    let sink = Reasoner::new(
        program,
        Input::Ignored,
        Output::Captured,
        ReasonerOptions {
            other: sink_args,
            ..Default::default()
        },
    );
    let mut stream = Pipeline::new(source).pipe(sink).execute().await.unwrap();
    let mut sockets = Vec::new();
    for _ in 0..2 {
        let (mut socket, _) = listener.accept().await.unwrap();
        assert_eq!(socket.read_u8().await.unwrap(), 1);
        sockets.push(socket);
    }
    if poll {
        assert_eq!(
            stream.next().await.unwrap().unwrap().into_lines(),
            vec![RECORD.to_vec()]
        );
    }
    drop(stream);
    for mut socket in sockets {
        let mut bytes = Vec::new();
        socket.read_to_end(&mut bytes).await.unwrap();
        assert!(bytes.is_empty());
    }
}

async fn stage_failure(program: &Path, middle: bool) {
    let source = reader(
        program,
        Input::Ignored,
        if middle { "hang" } else { "fail" },
    );
    let error = Pipeline::new(source)
        .pipe(reasoner(program, if middle { "fail" } else { "echo" }))
        .pipe(writer(program, "echo", Output::Captured))
        .execute()
        .await
        .unwrap_err();
    assert_eq!(error.stage, usize::from(middle));
    assert!(
        matches!(error.error, ExecutorError::Failure(_, Some(ref stderr)) if stderr == "fixture rejected input")
    );
}

async fn spawn_failure(program: &Path) {
    let source = reader(program, Input::Ignored, "hang");
    let sink = Writer::new(
        "asimov-pipeline-program-that-does-not-exist",
        Input::Ignored,
        Output::Captured,
        Default::default(),
    );
    let error = Pipeline::new(source)
        .pipe(sink)
        .execute()
        .await
        .unwrap_err();
    assert_eq!(error.stage, 1);
    assert!(matches!(error.error, ExecutorError::MissingProgram(_)));
}

async fn preflight(program: &Path) {
    let source = Reader::new(
        "asimov-pipeline-program-that-does-not-exist",
        Input::Ignored,
        Output::Captured,
        Default::default(),
    );
    let bad = Writer::new(
        program,
        Input::Ignored,
        Output::Captured,
        WriterOptions::builder().input("turtle").build(),
    );
    let error = Pipeline::new(source).pipe(bad).execute().await.unwrap_err();
    assert_eq!(error.stage, 1);
    assert!(
        matches!(error.error, ExecutorError::UnexpectedOther(ref e) if e.kind() == io::ErrorKind::InvalidInput)
    );

    let source = reader(program, Input::Ignored, "emit");
    let bad = Writer::new(
        program,
        Input::AsyncRead(Box::new(Cursor::new(RECORD))),
        Output::Captured,
        Default::default(),
    );
    let error = Pipeline::new(source).pipe(bad).execute().await.unwrap_err();
    assert_eq!(error.stage, 1);
    assert!(
        matches!(error.error, ExecutorError::UnexpectedOther(ref e) if e.kind() == io::ErrorKind::InvalidInput)
    );

    let source = Reader::new(
        program,
        Input::Ignored,
        Output::AsyncWrite(Box::new(tokio::io::sink())),
        Default::default(),
    );
    let error = Pipeline::new(source)
        .pipe(writer(program, "echo", Output::Captured))
        .execute()
        .await
        .unwrap_err();
    assert_eq!(error.stage, 0);
}

async fn limited_lister(program: &Path) {
    let mut source_args = args("emit-hang");
    source_args.push("--count=10".into());
    let source = Lister::new(
        program,
        "example:",
        Output::Captured,
        ListerOptions {
            limit: Some(2),
            other: source_args,
            ..Default::default()
        },
    );
    let bytes = Pipeline::new(source)
        .pipe(writer(program, "echo", Output::Captured))
        .execute()
        .await
        .unwrap()
        .into_inner();
    assert_eq!(bytes, RECORD.repeat(2));

    let source = Lister::new(
        "asimov-pipeline-program-that-does-not-exist",
        "example:",
        Output::Captured,
        ListerOptions::builder().limit(0).build(),
    );
    let bytes = Pipeline::new(source)
        .pipe(writer(program, "echo", Output::Captured))
        .execute()
        .await
        .unwrap()
        .into_inner();
    assert!(bytes.is_empty());

    let source = Lister::new(
        program,
        "example:",
        Output::Captured,
        ListerOptions {
            limit: Some(3),
            other: args("fail"),
            ..Default::default()
        },
    );
    let error = Pipeline::new(source)
        .pipe(writer(program, "echo", Output::Captured))
        .execute()
        .await
        .unwrap_err();
    assert_eq!(
        error.stage, 0,
        "relay source errors must retain the lister's stage index"
    );
    assert!(
        matches!(error.error, ExecutorError::Failure(_, Some(ref stderr)) if stderr == "fixture rejected input")
    );
}

async fn input_failure(program: &Path) {
    let source = futures_lite::stream::iter([Err(ExecutorError::UnexpectedOther(
        io::Error::other("source failed"),
    ))]);
    let producer = Reasoner::new(
        program,
        Input::Jsonl(Box::pin(source)),
        Output::Captured,
        ReasonerOptions {
            other: args("echo"),
            ..Default::default()
        },
    );
    let error = Pipeline::new(producer)
        .pipe(writer(program, "echo", Output::Captured))
        .execute()
        .await
        .unwrap_err();
    assert_eq!(error.stage, 0);
    assert!(
        matches!(error.error, ExecutorError::UnexpectedOther(ref error) if error.to_string() == "source failed")
    );
}

struct FailingWriter;
impl AsyncWrite for FailingWriter {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, _: &[u8]) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::Error::other("destination failed")))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

async fn output_failure(program: &Path) {
    let source = reader(program, Input::Ignored, "emit");
    let error = Pipeline::new(source)
        .pipe(writer(
            program,
            "echo",
            Output::AsyncWrite(Box::new(FailingWriter)),
        ))
        .execute()
        .await
        .unwrap_err();
    assert_eq!(error.stage, 1);
    assert!(
        matches!(error.error, ExecutorError::UnexpectedOther(ref error) if error.to_string() == "destination failed")
    );
}

async fn forwarded_graph(program: &Path) {
    let source = reader(program, Input::Ignored, "emit");
    let tail = Reasoner::new(
        program,
        Input::Ignored,
        Output::AsyncWrite(Box::new(FailingWriter)),
        ReasonerOptions {
            other: args("echo"),
            ..Default::default()
        },
    );
    let mut stream = Pipeline::new(source).pipe(tail).execute().await.unwrap();
    let error = stream.next().await.unwrap().unwrap_err();
    assert_eq!(error.stage, 1);
    assert!(
        matches!(error.error, ExecutorError::UnexpectedOther(ref error) if error.to_string() == "destination failed")
    );
    assert!(stream.next().await.is_none());
}

async fn output_policies(program: &Path) {
    let (destination, mut receive) = tokio::io::duplex(8);
    let source = reader(program, Input::Ignored, "emit");
    let tail = Reasoner::new(
        program,
        Input::Ignored,
        Output::AsyncWrite(Box::new(destination)),
        ReasonerOptions {
            other: args("echo"),
            ..Default::default()
        },
    );
    let mut stream = Pipeline::new(source).pipe(tail).execute().await.unwrap();
    let (_, bytes) = tokio::join!(
        async {
            assert!(stream.next().await.is_none());
        },
        async {
            let mut bytes = Vec::new();
            receive.read_to_end(&mut bytes).await.unwrap();
            bytes
        }
    );
    assert_eq!(bytes, RECORD);

    let source = reader(program, Input::Ignored, "emit");
    let tail = Reasoner::new(
        program,
        Input::Ignored,
        Output::Ignored,
        ReasonerOptions {
            other: args("echo"),
            ..Default::default()
        },
    );
    assert!(
        Pipeline::new(source)
            .pipe(tail)
            .execute()
            .await
            .unwrap()
            .next()
            .await
            .is_none()
    );
}

async fn graph_failure(program: &Path) {
    let source = reader(program, Input::Ignored, "fail");
    let mut stream = Pipeline::new(source)
        .pipe(reasoner(program, "echo"))
        .execute()
        .await
        .unwrap();
    let error = stream.next().await.unwrap().unwrap_err();
    assert_eq!(error.stage, 0);
    assert!(
        matches!(error.error, ExecutorError::Failure(_, Some(ref stderr)) if stderr == "fixture rejected input")
    );
    assert!(stream.next().await.is_none());
}

async fn single_stage(program: &Path) {
    let mut stream = Pipeline::new(reader(program, Input::Ignored, "emit"))
        .execute()
        .await
        .unwrap();
    assert_eq!(
        stream.next().await.unwrap().unwrap().into_lines(),
        vec![RECORD.to_vec()]
    );
    assert!(stream.next().await.is_none());
    let lister = Lister::new(
        program,
        "example:",
        Output::Captured,
        ListerOptions {
            limit: Some(1),
            other: args("emit-hang"),
            ..Default::default()
        },
    );
    let mut stream = Pipeline::new(lister).execute().await.unwrap();
    assert_eq!(
        stream.next().await.unwrap().unwrap().into_lines(),
        vec![RECORD.to_vec()]
    );
    assert!(stream.next().await.is_none());
}

async fn configured_batches(program: &Path) {
    for override_tail in [false, true] {
        let mut source_args = args("emit");
        source_args.push("--count=5".into());
        let source = Emitter::new(
            program,
            Output::Captured,
            EmitterOptions {
                other: source_args,
                ..Default::default()
            },
        );
        let tail = reasoner(program, "echo")
            .with_batching(BatchOptions::new(2, 1024, Duration::from_secs(1)).unwrap());
        let pipeline = if override_tail {
            Pipeline::new(source)
                .with_batching(BatchOptions::new(3, 1024, Duration::from_secs(1)).unwrap())
                .pipe(tail)
        } else {
            Pipeline::new(source).pipe(tail)
        };
        let mut batches = pipeline.execute().await.unwrap();
        let expected = if override_tail {
            vec![3, 2]
        } else {
            vec![2, 2, 1]
        };
        for count in expected {
            let batch = batches.next().await.unwrap().unwrap();
            assert_eq!(batch.len(), count);
            assert_eq!(batch.byte_len(), count * RECORD.len());
            for line in batch.lines() {
                assert_eq!(line, RECORD);
            }
        }
        assert!(batches.next().await.is_none());
    }
}

async fn limited_batches(program: &Path) {
    let mut source_args = args("emit-hang");
    source_args.push("--count=10".into());
    let lister = Lister::new(
        program,
        "example:",
        Output::Captured,
        ListerOptions {
            limit: Some(3),
            other: source_args,
            ..Default::default()
        },
    )
    .with_batching(BatchOptions::new(2, 1024, Duration::from_secs(30)).unwrap())
    .with_capabilities(ListerCapabilities::default());
    // The tail inherits the lister's policy even after capability reconfiguration.
    // The second partial batch flushes at the line cap, not after the 30 s delay.
    let mut batches = Pipeline::new(lister).execute().await.unwrap();
    assert_eq!(batches.next().await.unwrap().unwrap().len(), 2);
    assert_eq!(batches.next().await.unwrap().unwrap().len(), 1);
    assert!(batches.next().await.is_none());
}
