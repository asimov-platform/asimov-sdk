// This is free and unencumbered software released into the public domain.

//! Linear pipelines of graph-compatible programs.
//!
//! [`Pipeline::new`] consumes a configured program; [`Pipeline::pipe`] appends a
//! graph consumer. Connections carry JSON-LD over JSONL by contract, without
//! parsing or transcoding. Native edges use cross-platform [`std::io::pipe`]
//! handles connected directly to child stdin/stdout: no shell, platform-specific
//! handle code, or full intermediate-output buffer is involved. Tokio drives
//! boundary I/O, stderr draining, and process supervision.
//!
//! # Reader → Writer
//!
//! ```no_run
//! use asimov_runner::{AnyInput, AnyOutput, GraphInput, GraphOutput, Pipeline, Reader, Writer};
//! use std::io::Cursor;
//!
//! # async fn example() -> Result<(), asimov_runner::PipelineError> {
//! let reader = Reader::new(
//!     "asimov-example-reader",
//!     AnyInput::AsyncRead(Box::new(Cursor::new(b"source document".to_vec()))),
//!     GraphOutput::Captured,
//!     Default::default(),
//! );
//! let writer = Writer::new(
//!     "asimov-example-writer", GraphInput::Ignored, AnyOutput::Captured, Default::default(),
//! );
//! let bytes = Pipeline::new(reader).pipe(writer).execute().await?.into_inner();
//! # Ok(())
//! # }
//! ```
//!
//! # Fetcher → Reasoner → Indexer
//!
//! ```no_run
//! use asimov_runner::{Fetcher, GraphInput, GraphOutput, Indexer, IndexerOptions, Pipeline, Reasoner};
//!
//! # async fn example() -> Result<(), asimov_runner::PipelineError> {
//! let fetcher = Fetcher::new(
//!     "asimov-example-fetcher", "https://example.com/resource",
//!     GraphOutput::Captured, Default::default(),
//! );
//! let reasoner = Reasoner::new(
//!     "asimov-example-reasoner", GraphInput::Ignored, GraphOutput::Captured, Default::default(),
//! );
//! let indexer = Indexer::new(
//!     "asimov-example-indexer", GraphInput::Ignored,
//!     IndexerOptions::builder().other("./catalog.index").build(),
//! );
//! Pipeline::new(fetcher).pipe(reasoner).pipe(indexer).execute().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Routing and completion
//!
//! A pipeline is a one-shot owner of its configured programs. Only the first
//! program supplies external input; later programs must use `Input::Ignored` as
//! the placeholder replaced by the connection. Intermediate stdout routing is
//! replaced by pipes; an intermediate `Output::AsyncWrite` is rejected rather
//! than silently discarding its writer. The final program's output policy is
//! honored. Explicit graph format options must be `jsonl` (or left unset).
//! Programs must agree on a JSON-LD profile and use the connected standard streams;
//! arbitrary `other` arguments and file operands are not interpreted by this API.
//!
//! Graph-producing tails return a live [`PipelineStream`] of [`crate::JsonlBatch`]
//! values. [`Pipeline::with_batching`] overrides the tail program's default
//! batching policy; intermediate native pipe edges remain byte streams. A [`Writer`] tail
//! returns buffered arbitrary-format bytes, and an [`Indexer`] tail returns `()`.
//! Success requires every stage to complete successfully, not just the tail.
//! Failures include their zero-based stage index and executable. The first
//! observed failure is reported, preferring downstream stages when multiple
//! outcomes are ready. Other directly supervised children are terminated and reaped before
//! returning that failure. Dropping execution or its stream requests termination
//! through each owned child's kill-on-drop policy; it does not synchronously reap
//! children, terminate arbitrary descendants, or roll back external side effects.
//!
//! All stages spawn before output is consumed, and command-owned copies of pipe
//! endpoints are released immediately after spawning. Polling the execution or
//! returned stream drives supervision and boundary I/O; no detached tasks are
//! created. Stderr and buffered final output have no configured size bound.
//! Graph batch sizes and collection delay follow [`BatchOptions`]. EOF flushes
//! a partial batch. Already-read complete lines are delivered before a terminal
//! error, without delaying cleanup once that error is observed.
//!
//! A limited [`Lister`] applies its line cap before producing batches at the first edge
//! so native pipe wiring cannot bypass the runner's limit. That edge is relayed
//! with backpressure through `Input::Jsonl` (which terminates unterminated input
//! lines with LF); other edges remain direct OS pipes. A zero-limit lister starts
//! no source child and supplies EOF downstream. Reaching its limit intentionally
//! cancels that source without checking its eventual exit status, as for standalone
//! lister execution, including its kill-on-drop/reaping policy. Native pipes
//! otherwise preserve bytes exactly. Neither a
//! successful exit nor writing to a pipe proves application-level processing.
//!
//! # Batch-oriented postprocessing
//!
//! ```no_run
//! use asimov_runner::{BatchOptions, Fetcher, GraphOutput, Pipeline, StreamExt};
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), asimov_runner::PipelineError> {
//! let fetcher = Fetcher::new(
//!     "asimov-example-fetcher", "https://example.com/resource",
//!     GraphOutput::Captured, Default::default(),
//! );
//! let policy = BatchOptions::new(128, 64 * 1024, Duration::from_millis(5))
//!     .expect("nonzero thresholds");
//! let mut batches = Pipeline::new(fetcher).with_batching(policy).execute().await?;
//! while let Some(batch) = batches.next().await {
//!     let batch = batch?;
//!     // Submit the whole batch to a network service, or iterate batch.lines().
//! }
//! # Ok(())
//! # }
//! ```

use crate::{
    BatchOptions, BatchStream, Executor, ExecutorError, Indexer, Input, InputCompletion,
    LineStream, Lister, Output, StreamExt, Writer, batch_lines,
};
use alloc::{boxed::Box, vec, vec::Vec};
use core::{
    fmt,
    future::{Future, poll_fn},
    marker::PhantomData,
    pin::Pin,
    task::Poll,
};
use std::{
    ffi::OsString,
    io::{self, Cursor},
    process::Stdio,
};
use tokio::{process::Child, sync::watch};

/// A pipeline failure attributed to a configured program.
#[derive(Debug)]
pub struct PipelineError {
    /// Zero-based stage index, in construction order.
    pub stage: usize,
    /// Executable name or resolved path used for that stage.
    pub program: OsString,
    /// Configuration, spawn, transport, input, or exit failure.
    pub error: ExecutorError,
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Pipeline stage {} ({}): {}",
            self.stage,
            self.program.to_string_lossy(),
            self.error
        )
    }
}

impl core::error::Error for PipelineError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// Live final graph batches; consume to EOF to check every stage's outcome.
/// Dropping the stream requests termination of all owned children.
pub type PipelineStream = BatchStream<PipelineError>;

mod sealed {
    pub trait Sealed {}
}

/// A built-in program that can participate in a linear graph pipeline.
/// This trait is sealed; use the configured runner types to construct stages.
pub trait PipelineProgram: sealed::Sealed + Into<PipelineStage> {}

/// A program whose output can supply a graph consumer's stdin.
pub trait GraphProducer: PipelineProgram {}

/// A program accepting JSONL graph input from a preceding stage.
pub trait GraphConsumer: PipelineProgram {}

macro_rules! programs {
    (producer: $($producer:ty),*; consumer: $($consumer:ty),*; both: $($both:ty),*) => {
        $(impl sealed::Sealed for $producer {}
          impl PipelineProgram for $producer {}
          impl GraphProducer for $producer {})*
        $(impl sealed::Sealed for $consumer {}
          impl PipelineProgram for $consumer {}
          impl GraphConsumer for $consumer {})*
        $(impl sealed::Sealed for $both {}
          impl PipelineProgram for $both {}
          impl GraphProducer for $both {}
          impl GraphConsumer for $both {})*
    };
}
programs! {
    producer: crate::Adapter, crate::Emitter, crate::Fetcher, Lister, crate::Reader;
    consumer: Writer, Indexer;
    both: crate::Matcher, crate::Reasoner
}

/// An owned, nonempty linear pipeline, typed by its final program.
///
/// Only graph producers can be followed by graph consumers; writer and indexer
/// tails terminate construction. For example, a fetcher cannot consume graph input:
///
/// ```compile_fail
/// use asimov_runner::{Fetcher, GraphOutput, Pipeline};
/// let a = Fetcher::new("a", "example:", GraphOutput::Captured, Default::default());
/// let b = Fetcher::new("b", "example:", GraphOutput::Captured, Default::default());
/// let invalid = Pipeline::new(a).pipe(b);
/// ```
///
/// A writer is a terminal, arbitrary-format output stage, not a graph producer:
///
/// ```compile_fail
/// use asimov_runner::{Input, Output, Pipeline, Reasoner, Writer};
/// let writer = Writer::new("writer", Input::Ignored, Output::Captured, Default::default());
/// let reasoner = Reasoner::new("reasoner", Input::Ignored, Output::Captured, Default::default());
/// let invalid = Pipeline::new(writer).pipe(reasoner);
/// ```
#[derive(Debug)]
pub struct Pipeline<P> {
    stages: Vec<PipelineStage>,
    batching: Option<BatchOptions>,
    tail: PhantomData<fn() -> P>,
}

impl<P: PipelineProgram> Pipeline<P> {
    /// Starts a pipeline without spawning processes. Consumes the program and
    /// preserves its arguments, source input, and final-output configuration.
    pub fn new(program: P) -> Self {
        Self {
            stages: vec![program.into()],
            batching: None,
            tail: PhantomData,
        }
    }
}

impl<P: GraphProducer> Pipeline<P> {
    /// Appends a graph consumer. Its `Input::Ignored` is replaced by a pipe from
    /// the preceding program. No processes are started during construction.
    pub fn pipe<N: GraphConsumer>(mut self, program: N) -> Pipeline<N> {
        self.stages.push(program.into());
        Pipeline {
            stages: self.stages,
            batching: self.batching,
            tail: PhantomData,
        }
    }

    /// Overrides batching for the final Rust-facing graph stream. Native pipe
    /// edges are unchanged. This override survives `pipe` calls; otherwise the
    /// final program's batching policy is used. Byte/unit tails do not batch output.
    #[must_use]
    pub fn with_batching(mut self, options: BatchOptions) -> Self {
        self.batching = Some(options);
        self
    }

    /// Spawns the pipeline and returns final JSONL batches without waiting for exit.
    /// Configuration/launch errors are returned directly; later failures are
    /// final stream items after any buffered complete lines. Non-captured final
    /// output yields no payload batches.
    pub async fn execute(self) -> Result<PipelineStream, PipelineError> {
        let batching = self
            .batching
            .unwrap_or(self.stages.last().unwrap().batching);
        Ok(batch_lines(self.execute_lines().await?, batching))
    }

    async fn execute_lines(self) -> Result<LineStream<PipelineError>, PipelineError> {
        let mut running = start(self.stages, true).await?;
        let mut lines = running.lines.take();
        let tail = running.tail.clone();
        Ok(Box::pin(async_stream::try_stream! {
            let mut completed = false;
            if let Some(ref mut lines) = lines {
                loop {
                    let event = tokio::select! {
                        result = running.wait(), if !completed => {
                            completed = true;
                            result.map(|_| None)
                        },
                        line = lines.next() => Ok(Some(line)),
                    }?;
                    let Some(line) = event else { continue };
                    match line {
                        Some(Ok(line)) => yield line,
                        Some(Err(error)) => {
                            running.cancel().await;
                            Err(tail.error(error))?;
                        },
                        None => break,
                    }
                }
            }
            if !completed {
                running.wait().await?;
            }
        }))
    }
}

impl Pipeline<Writer> {
    /// Runs every stage and returns the writer's captured arbitrary-format bytes.
    /// Forwarded, ignored, and inherited stdout return an empty cursor. Any stage
    /// failure fails the pipeline, even if the writer exits successfully.
    pub async fn execute(self) -> Result<Cursor<Vec<u8>>, PipelineError> {
        let mut running = start(self.stages, false).await?;
        Ok(Cursor::new(running.wait().await?))
    }
}

impl Pipeline<Indexer> {
    /// Runs every stage and waits for successful indexing and upstream completion.
    pub async fn execute(self) -> Result<(), PipelineError> {
        start(self.stages, false).await?.wait().await?;
        Ok(())
    }
}

/// Opaque owned stage configuration used by the sealed pipeline traits.
#[doc(hidden)]
#[derive(Debug)]
pub struct PipelineStage {
    program: OsString,
    kind: StageKind,
    error: Option<ExecutorError>,
    external_input: bool,
    external_writer: bool,
    batching: BatchOptions,
}

#[derive(Debug)]
enum StageKind {
    Native {
        executor: Executor,
        input: Input,
        output: Output,
    },
    LimitedLister(Box<Lister>),
}

impl PipelineStage {
    pub(crate) fn native(
        mut executor: Executor,
        input: Input,
        output: Output,
        error: Option<ExecutorError>,
    ) -> Self {
        Self {
            program: executor.command().as_std().get_program().to_os_string(),
            batching: executor.batch_options(),
            external_input: !matches!(input, Input::Ignored),
            external_writer: matches!(output, Output::AsyncWrite(_)),
            kind: StageKind::Native {
                executor,
                input,
                output,
            },
            error,
        }
    }

    pub(crate) fn limited_lister(
        lister: Lister,
        program: OsString,
        external_writer: bool,
        error: Option<ExecutorError>,
        batching: BatchOptions,
    ) -> Self {
        Self {
            program,
            kind: StageKind::LimitedLister(Box::new(lister)),
            error,
            external_input: false,
            external_writer,
            batching,
        }
    }
}

pub(crate) fn graph_formats(input: Option<&str>, output: Option<&str>) -> Option<ExecutorError> {
    for (option, format) in [("input", input), ("output", output)] {
        if let Some(format) = format {
            if format != "jsonl" {
                return Some(invalid(alloc::format!(
                    "pipeline graph {option} format must be jsonl, got {format}"
                )));
            }
        }
    }
    None
}

fn invalid(message: impl Into<alloc::string::String>) -> ExecutorError {
    io::Error::new(io::ErrorKind::InvalidInput, message.into()).into()
}

macro_rules! stage {
    ($program:ty, $value:ident, $input:expr, $output:expr, $input_format:expr, $output_format:expr) => {
        impl From<$program> for crate::pipeline::PipelineStage {
            fn from($value: $program) -> Self {
                let error = crate::pipeline::graph_formats($input_format, $output_format);
                Self::native($value.executor, $input, $output, error)
            }
        }
    };
}
pub(crate) use stage;

#[derive(Clone)]
struct StageInfo {
    index: usize,
    program: OsString,
}

impl StageInfo {
    fn error(&self, error: impl Into<ExecutorError>) -> PipelineError {
        PipelineError {
            stage: self.index,
            program: self.program.clone(),
            error: error.into(),
        }
    }
}

type Job = Pin<Box<dyn Future<Output = Result<Vec<u8>, PipelineError>> + Send>>;

struct Running {
    jobs: Vec<Option<Job>>,
    stop: watch::Sender<bool>,
    lines: Option<LineStream>,
    tail: StageInfo,
    failure: Option<PipelineError>,
    output: Vec<u8>,
}

impl Running {
    // State lives in Running, not this future: selecting on wait while reading
    // final output can cancel the future without losing completed jobs or errors.
    async fn next(&mut self) -> Option<(usize, Result<Vec<u8>, PipelineError>)> {
        poll_fn(|cx| {
            let mut pending = false;
            for (index, job) in self.jobs.iter_mut().enumerate().rev() {
                if let Some(future) = job {
                    match future.as_mut().poll(cx) {
                        Poll::Ready(result) => {
                            *job = None;
                            return Poll::Ready(Some((index, result)));
                        },
                        Poll::Pending => pending = true,
                    }
                }
            }
            if pending {
                Poll::Pending
            } else {
                Poll::Ready(None)
            }
        })
        .await
    }

    async fn wait(&mut self) -> Result<Vec<u8>, PipelineError> {
        while let Some((index, result)) = self.next().await {
            match result {
                Ok(bytes) if index + 1 == self.jobs.len() => self.output = bytes,
                Ok(_) => {},
                Err(error) if self.failure.is_none() => {
                    self.failure = Some(error);
                    self.stop.send_replace(true);
                },
                Err(_) => {},
            }
        }
        match self.failure.take() {
            Some(error) => Err(error),
            None => Ok(core::mem::take(&mut self.output)),
        }
    }

    async fn cancel(&mut self) {
        self.stop.send_replace(true);
        while self.next().await.is_some() {}
    }
}

struct Spawned {
    child: Child,
    input: Input,
    output: Output,
    info: StageInfo,
}

async fn run_stage(
    mut stage: Spawned,
    mut stop: watch::Receiver<bool>,
    source: Option<StageInfo>,
) -> Result<Vec<u8>, PipelineError> {
    let cancelled = *stop.borrow();
    let completion = if cancelled {
        None
    } else {
        tokio::select! {
            biased;
            result = crate::executor::communicate_child(&mut stage.child, &mut stage.input, &mut stage.output) => Some(result),
            _ = stop.changed() => None,
        }
    };
    match completion {
        Some(result) => {
            let completion = result.map_err(|error| stage.info.error(error))?;
            let info = if matches!(completion.input, InputCompletion::SourceFailed(_)) {
                source.as_ref().unwrap_or(&stage.info)
            } else {
                &stage.info
            };
            completion
                .into_result()
                .map(Cursor::into_inner)
                .map_err(|error| info.error(error))
        },
        None => {
            let _ = stage.child.start_kill();
            let _ = stage.child.wait().await;
            Ok(Vec::new())
        },
    }
}

async fn start(
    mut stages: Vec<PipelineStage>,
    capture_graph: bool,
) -> Result<Running, PipelineError> {
    let count = stages.len();
    // Validate the entire chain before starting any process or consuming a source.
    for (index, stage) in stages.iter_mut().enumerate() {
        let info = StageInfo {
            index,
            program: stage.program.clone(),
        };
        if let Some(error) = stage.error.take() {
            return Err(info.error(error));
        }
        if index != 0 && stage.external_input {
            return Err(info.error(invalid(
                "piped stages must use Input::Ignored; their input comes from the preceding stage",
            )));
        }
        if index + 1 != count && stage.external_writer {
            return Err(info.error(invalid("an intermediate pipeline stage cannot also forward stdout to an AsyncWrite destination")));
        }
    }
    let tail = StageInfo {
        index: count - 1,
        program: stages.last().unwrap().program.clone(),
    };
    let (stop, receiver) = watch::channel(false);
    let mut running = Running {
        jobs: Vec::new(),
        stop,
        lines: None,
        tail,
        failure: None,
        output: Vec::new(),
    };
    let mut source_info = None;
    let mut limited_source = None;
    if matches!(stages[0].kind, StageKind::LimitedLister(_)) {
        let stage = stages.remove(0);
        let info = StageInfo {
            index: 0,
            program: stage.program,
        };
        let StageKind::LimitedLister(mut lister) = stage.kind else {
            unreachable!()
        };
        if stages.is_empty() {
            running.lines = Some(
                lister
                    .execute_lines()
                    .await
                    .map_err(|error| info.error(error))?,
            );
            return Ok(running);
        }
        limited_source = Some(lister);
        source_info = Some(info);
    }
    let base = usize::from(source_info.is_some());
    for index in 0..stages.len().saturating_sub(1) {
        let (reader, writer) = io::pipe().map_err(|error| {
            StageInfo {
                index: index + base,
                program: stages[index].program.clone(),
            }
            .error(error)
        })?;
        let StageKind::Native { executor, .. } = &mut stages[index].kind else {
            unreachable!()
        };
        executor.command().stdout(Stdio::from(writer));
        let StageKind::Native { executor, .. } = &mut stages[index + 1].kind else {
            unreachable!()
        };
        executor.command().stdin(Stdio::from(reader));
    }
    if let Some(lister) = limited_source {
        let source = lister
            .into_pipeline_source()
            .await
            .map_err(|error| source_info.as_ref().unwrap().error(error))?;
        let StageKind::Native {
            executor, input, ..
        } = &mut stages[0].kind
        else {
            unreachable!()
        };
        *input = Input::Jsonl(source);
        executor.command().stdin(input.as_stdio());
    }
    let mut spawned: Vec<Spawned> = Vec::new();
    let mut plans = stages.into_iter().enumerate();
    while let Some((index, stage)) = plans.next() {
        let info = StageInfo {
            index: index + base,
            program: stage.program,
        };
        let StageKind::Native {
            mut executor,
            input,
            mut output,
        } = stage.kind
        else {
            unreachable!()
        };
        if info.index + 1 != count {
            output = Output::Ignored;
        }
        let result = executor.spawn().await;
        // Command stores pipe handles too. Keeping it alive would prevent EOF.
        drop(executor);
        let mut child = match result {
            Ok(child) => child,
            Err(error) => {
                drop(plans);
                drop(input);
                for stage in &mut spawned {
                    stage.input = Input::Ignored;
                    let _ = stage.child.start_kill();
                }
                for stage in &mut spawned {
                    let _ = stage.child.wait().await;
                }
                return Err(info.error(error));
            },
        };
        if info.index + 1 == count && capture_graph && matches!(output, Output::Captured) {
            running.lines = child.stdout.take().map(crate::jsonl_lines);
        }
        spawned.push(Spawned {
            child,
            input,
            output,
            info,
        });
    }
    for (index, stage) in spawned.into_iter().enumerate() {
        running.jobs.push(Some(Box::pin(run_stage(
            stage,
            receiver.clone(),
            if index == 0 {
                source_info.clone()
            } else {
                None
            },
        ))));
    }
    Ok(running)
}
