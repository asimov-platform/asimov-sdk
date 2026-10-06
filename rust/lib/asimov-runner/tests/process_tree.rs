// This is free and unencumbered software released into the public domain.

// A portable fixture creates a child and grandchild, each retaining a TCP
// connection. EOF/reset proves termination independently of captured pipes.
extern crate alloc;

use alloc::{boxed::Box, format, string::String, vec, vec::Vec};
use asimov_runner::{
    BatchOptions, Executor, ExecutorError, Fetcher, GraphOutput, Input, Lister, ListerCapabilities,
    ListerOptions, OptionSupport, Pipeline, StreamExt, Writer, WriterOptions,
};
use core::time::Duration;
use std::{
    env,
    io::{self, Read, Write},
    process::{Command, Stdio},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

fn main() {
    let args: Vec<_> = env::args().collect();
    if let Some(index) = args.iter().position(|arg| arg == "--tree-fixture") {
        fixture(args[index + 1].parse().unwrap(), &args[index + 2]).unwrap();
        return;
    }
    if args.iter().any(|arg| arg == "--copy") {
        io::copy(&mut io::stdin().lock(), &mut io::stdout().lock()).unwrap();
        return;
    }
    #[cfg(any(unix, windows))]
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(tests());
}

fn fixture(depth: u8, address: &str) -> io::Result<()> {
    let descendant = if depth > 0 {
        let mut child = Command::new(env::current_exe()?)
            .args(["--tree-fixture", &format!("{}", depth - 1), address])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()?;
        // Do not emit the line that triggers a lister cap until the whole tree
        // is ready. Descendants retain inherited stderr, reproducing a common
        // source of hung cancellation when only the root is killed.
        let mut ready = [0; 3];
        child.stdout.as_mut().unwrap().read_exact(&mut ready)?;
        assert_eq!(&ready, b"{}\n");
        Some(child)
    } else {
        None
    };
    let mut socket = std::net::TcpStream::connect(address)?;
    socket.write_all(&[depth])?;
    socket.write_all(&std::process::id().to_be_bytes())?;
    io::stdout().write_all(b"{}\n")?;
    io::stdout().flush()?;
    // Bound fixture lifetime even if a regression makes its test panic.
    socket.set_read_timeout(Some(Duration::from_secs(20)))?;
    let _ = socket.read_exact(&mut [0]);
    // Intentionally exit without waiting: normal leader-exit cleanup must
    // still cover descendants, including those that inherited the pipes.
    drop(descendant);
    Ok(())
}

struct Tree {
    listener: TcpListener,
    members: Vec<(u8, u32, TcpStream)>,
}

impl Tree {
    async fn new() -> Self {
        Self {
            listener: TcpListener::bind("127.0.0.1:0").await.unwrap(),
            members: Vec::new(),
        }
    }

    fn args(&self) -> Vec<String> {
        vec![
            "--tree-fixture".into(),
            "2".into(),
            format!("{}", self.listener.local_addr().unwrap()),
        ]
    }

    fn executor(&self, owned: bool) -> Executor {
        let mut executor = Executor::new(env::current_exe().unwrap());
        executor.command().args(self.args());
        executor.capture_stdout();
        executor.capture_stderr();
        if owned {
            executor.with_process_tree()
        } else {
            executor
        }
    }

    fn fetcher(&self) -> Fetcher {
        Fetcher::new_with_args(
            env::current_exe().unwrap(),
            self.args(),
            "example:resource",
            GraphOutput::Captured,
            Default::default(),
        )
        .with_process_tree()
        .with_batching(BatchOptions::new(1, 1024, Duration::from_millis(1)).unwrap())
    }

    async fn ready(&mut self) {
        for _ in 0..3 {
            let (mut socket, _) = self.listener.accept().await.unwrap();
            let depth = socket.read_u8().await.unwrap();
            let pid = socket.read_u32().await.unwrap();
            self.members.push((depth, pid, socket));
        }
        self.members.sort_by_key(|(depth, _, _)| *depth);
        assert_eq!(
            self.members.iter().map(|(d, _, _)| *d).collect::<Vec<_>>(),
            [0, 1, 2]
        );
    }

    async fn terminated(&mut self) {
        for (_, _, socket) in &mut self.members {
            closed(socket).await;
        }
        #[cfg(unix)]
        {
            use rustix::process::{Pid, WaitId, WaitIdOptions, waitid};
            let root = Pid::from_raw(self.members[2].1 as i32).unwrap();
            // NOWAIT observes without stealing the runner's wait status. EOF
            // alone would miss an unreaped direct-child zombie.
            loop {
                match waitid(
                    WaitId::Pid(root),
                    WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
                ) {
                    Err(rustix::io::Errno::CHILD) => break,
                    Ok(_) => tokio::time::sleep(Duration::from_millis(5)).await,
                    Err(error) => panic!("checking root reaping failed: {error}"),
                }
            }
        }
    }
}

async fn closed(socket: &mut TcpStream) {
    match socket.read_u8().await {
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
            ) => {},
        result => panic!("expected process connection to close, got {result:?}"),
    }
}

async fn tests() {
    macro_rules! case {
        ($test:expr) => {
            timeout(Duration::from_secs(10), $test)
                .await
                .expect(stringify!($test));
            std::println!("{} ... ok", stringify!($test));
        };
    }
    case!(deadline_cancels_buffered_execution());
    case!(dropping_fetch_stream(false));
    case!(dropping_fetch_stream(true));
    case!(lister_cap_preserves_ownership());
    case!(pipeline_preserves_ownership());
    case!(explicit_kill_and_wait());
    case!(leader_exit_cleans_descendants(false));
    case!(leader_exit_cleans_descendants(true));
    case!(default_remains_direct_child_only());
    case!(cancellation_is_isolated());
    case!(spawn_failure_and_legacy_spawn());
    case!(repeated_successful_execution());
}

async fn deadline_cancels_buffered_execution() {
    let mut tree = Tree::new().await;
    let mut executor = tree.executor(true);
    let mut execution = Box::pin(executor.execute());
    tokio::select! {
        result = &mut execution => panic!("execution completed early: {result:?}"),
        _ = tree.ready() => {},
    }
    assert!(timeout(Duration::from_millis(20), execution).await.is_err());
    tree.terminated().await;
}

async fn dropping_fetch_stream(poll: bool) {
    let mut tree = Tree::new().await;
    let mut stream = tree.fetcher().execute().await.unwrap();
    tree.ready().await;
    if poll {
        assert_eq!(
            stream
                .next()
                .await
                .unwrap()
                .unwrap()
                .lines()
                .next()
                .unwrap(),
            b"{}\n"
        );
    }
    drop(stream);
    tree.terminated().await;
}

async fn lister_cap_preserves_ownership() {
    let mut tree = Tree::new().await;
    let mut lister: Lister = Lister::new_with_args(
        env::current_exe().unwrap(),
        tree.args(),
        "example:collection",
        GraphOutput::Captured,
        ListerOptions::builder().limit(1).build(),
    )
    .with_process_tree()
    .with_capabilities(
        ListerCapabilities::builder()
            .limit(OptionSupport::Unsupported)
            .build(),
    );
    let mut stream = lister.execute().await.unwrap();
    tree.ready().await;
    assert_eq!(stream.next().await.unwrap().unwrap().len(), 1);
    // Retain the capped stream while checking that cancellation is immediate.
    tree.terminated().await;
    assert!(stream.next().await.is_none());
}

async fn pipeline_preserves_ownership() {
    let mut tree = Tree::new().await;
    let writer = Writer::new(
        env::current_exe().unwrap(),
        Input::Ignored,
        GraphOutput::Captured,
        WriterOptions::builder().other("--copy").build(),
    );
    let mut execution = Box::pin(Pipeline::new(tree.fetcher()).pipe(writer).execute());
    tokio::select! {
        result = &mut execution => panic!("pipeline completed early: {result:?}"),
        _ = tree.ready() => {},
    }
    drop(execution);
    tree.terminated().await;
}

async fn explicit_kill_and_wait() {
    let mut tree = Tree::new().await;
    let mut child = tree.executor(true).spawn_owned().await.unwrap();
    tree.ready().await;
    assert_eq!(child.id(), Some(tree.members[2].1));
    assert!(child.try_wait().unwrap().is_none());
    child.kill().await.unwrap();
    let status = child.wait().await.unwrap();
    assert!(!status.success());
    assert_eq!(child.try_wait().unwrap(), Some(status));
    assert_eq!(child.id(), None);
    tree.terminated().await;
}

async fn leader_exit_cleans_descendants(try_wait: bool) {
    let mut tree = Tree::new().await;
    let mut child = tree.executor(true).spawn_owned().await.unwrap();
    tree.ready().await;
    tree.members[2].2.write_all(b"exit").await.unwrap();
    let status = if try_wait {
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    } else {
        child.wait().await.unwrap()
    };
    assert!(status.success());
    tree.terminated().await;
    assert_eq!(child.wait().await.unwrap(), status);
}

async fn default_remains_direct_child_only() {
    let mut tree = Tree::new().await;
    let mut executor = tree.executor(false);
    assert!(!executor.owns_process_tree());
    let stream = executor.execute_jsonl().await.unwrap();
    tree.ready().await;
    drop(stream);
    closed(&mut tree.members[2].2).await;
    for (_, _, socket) in &mut tree.members[..2] {
        assert!(
            timeout(Duration::from_millis(30), socket.read_u8())
                .await
                .is_err()
        );
        socket.write_all(b"exit").await.unwrap();
    }
    tree.terminated().await;
}

async fn cancellation_is_isolated() {
    let mut first = Tree::new().await;
    let mut second = Tree::new().await;
    let one = first.executor(true).spawn_owned().await.unwrap();
    let two = second.executor(true).spawn_owned().await.unwrap();
    first.ready().await;
    second.ready().await;
    drop(one);
    first.terminated().await;
    for (_, _, socket) in &mut second.members {
        assert!(
            timeout(Duration::from_millis(30), socket.read_u8())
                .await
                .is_err()
        );
    }
    drop(two);
    second.terminated().await;
}

async fn spawn_failure_and_legacy_spawn() {
    let mut executor = Executor::new("asimov-nonexistent-tree-fixture").with_process_tree();
    assert!(matches!(
        executor.spawn_owned().await,
        Err(ExecutorError::MissingProgram(_))
    ));
    assert!(
        matches!(executor.spawn().await, Err(ExecutorError::SpawnFailure(error))
        if error.kind() == io::ErrorKind::InvalidInput)
    );
}

async fn repeated_successful_execution() {
    let mut executor = Executor::new(env::current_exe().unwrap()).with_process_tree();
    executor.command().arg("--copy");
    for _ in 0..3 {
        assert!(executor.execute().await.unwrap().into_inner().is_empty());
    }
}
