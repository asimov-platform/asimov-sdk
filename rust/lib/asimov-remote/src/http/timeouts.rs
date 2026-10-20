// This is free and unencumbered software released into the public domain.

//! Local transport budgets, independent of remote execution timing options.

use super::{BatchOptions, Error, JsonlStream};
use alloc::{boxed::Box, sync::Arc};
use asimov_flow::{Bytes, Stream, jsonl_batches_from_chunks};
use core::{
    fmt,
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll},
    time::Duration,
};
use tokio::time::{Instant, Sleep};

/// Local HTTP timeouts, configured with [`super::Executor::new_with_timeouts`].
///
/// Defaults: 10 s to connect, 60 s from dispatch to response headers, 60 s of
/// body inactivity, and 30 s for identity discovery. There is no total deadline
/// by default, so a progressing response can stream indefinitely.
///
/// `None` disables a limit. Enabled limits must be nonzero and representable
/// by the monotonic clock. Overlapping limits race; the first to expire wins.
/// These limits do not alter [`super::TimingOptions`] sent to the service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimeoutOptions {
    /// New-connection budget, including DNS, TCP, proxy negotiation, and TLS.
    /// Reused connections do not need this phase. Default: 10 seconds.
    pub connect: Option<Duration>,
    /// Absolute budget from dispatch until response headers arrive, including
    /// connection setup and uploading the buffered JSON request. This is not
    /// reset by partial headers or upload progress. Default: 60 seconds.
    pub response_headers: Option<Duration>,
    /// Maximum wait for nonempty response-body data or completion, including
    /// required trailers. Resets on data, and when the caller resumes after
    /// receiving a batch. Empty chunks do not reset it. Default: 60 seconds.
    pub body_idle: Option<Duration>,
    /// Total budget per identity lookup, including waiting for a shared lookup,
    /// connection setup, headers, and its complete body. Default: 30 seconds.
    /// A timed-out lookup never supplies a partial or stale identity.
    pub discovery: Option<Duration>,
    /// Optional wall-clock budget for an operation through body EOF. Starts
    /// when `post_jsonl` or `check` is first polled and includes discovery and
    /// downstream pauses. Expiry is observed while polling for body data;
    /// complete buffered records still precede the terminal timeout.
    /// Does not apply to standalone `remote_public_key` calls. Default: `None`.
    pub total: Option<Duration>,
}

impl Default for TimeoutOptions {
    fn default() -> Self {
        Self {
            connect: Some(Duration::from_secs(10)),
            response_headers: Some(Duration::from_secs(60)),
            body_idle: Some(Duration::from_secs(60)),
            discovery: Some(Duration::from_secs(30)),
            total: None,
        }
    }
}

impl TimeoutOptions {
    /// Disables all adapter timeout limits. OS/network limits still apply.
    pub const fn disabled() -> Self {
        Self {
            connect: None,
            response_headers: None,
            body_idle: None,
            discovery: None,
            total: None,
        }
    }

    pub(super) fn validate(self) -> Result<(), Error> {
        for (phase, limit) in [
            (TimeoutPhase::Connect, self.connect),
            (TimeoutPhase::ResponseHeaders, self.response_headers),
            (TimeoutPhase::BodyIdle, self.body_idle),
            (TimeoutPhase::Discovery, self.discovery),
            (TimeoutPhase::Total, self.total),
        ] {
            Deadline::new(limit, phase)?;
        }
        Ok(())
    }
}

/// The local transport budget that expired.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TimeoutPhase {
    /// DNS, TCP, proxy, or TLS connection setup.
    Connect,
    /// Dispatch/upload through receipt of response headers.
    ResponseHeaders,
    /// Waiting for response-body data, trailers, or EOF.
    BodyIdle,
    /// Remote identity lookup, including shared-lookup waits and body reads.
    Discovery,
    /// The optional whole-operation wall-clock budget.
    Total,
}

impl fmt::Display for TimeoutPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Connect => "connection setup",
            Self::ResponseHeaders => "request upload/response headers",
            Self::BodyIdle => "response-body inactivity",
            Self::Discovery => "identity discovery",
            Self::Total => "total operation",
        })
    }
}

#[derive(Clone, Copy)]
pub(super) struct Deadline {
    at: Instant,
    phase: TimeoutPhase,
    duration: Duration,
}

impl Deadline {
    pub(super) fn new(
        duration: Option<Duration>,
        phase: TimeoutPhase,
    ) -> Result<Option<Self>, Error> {
        duration
            .map(|duration| {
                let at = Instant::now()
                    .checked_add(duration)
                    .filter(|_| !duration.is_zero())
                    .ok_or(Error::InvalidTimeout(phase))?;
                Ok(Self {
                    at,
                    phase,
                    duration,
                })
            })
            .transpose()
    }

    fn error(self) -> Error {
        Error::Timeout {
            phase: self.phase,
            duration: self.duration,
        }
    }

    pub(super) fn check(self) -> Result<(), Error> {
        if Instant::now() >= self.at {
            return Err(self.error());
        }
        Ok(())
    }
}

pub(super) async fn until<F: Future>(
    deadline: Option<Deadline>,
    future: F,
) -> Result<F::Output, Error> {
    let Some(deadline) = deadline else {
        return Ok(future.await);
    };
    deadline.check()?;
    let output = tokio::time::timeout_at(deadline.at, future)
        .await
        .map_err(|_| deadline.error())?;
    // A synchronous poll can run past the deadline before yielding control.
    deadline.check()?;
    Ok(output)
}

pub(super) async fn limit<F: Future>(
    duration: Option<Duration>,
    phase: TimeoutPhase,
    future: F,
) -> Result<F::Output, Error> {
    until(Deadline::new(duration, phase)?, future).await
}

/// Watches verified or unverified bytes before JSONL framing, so complete
/// buffered records still precede a terminal timeout and partial lines do not.
pub(super) struct TimedStream<S> {
    inner: Option<Pin<Box<S>>>,
    idle: Option<Duration>,
    idle_timer: Option<Pin<Box<Sleep>>>,
    total: Option<(Deadline, Pin<Box<Sleep>>)>,
    reset_idle: Arc<AtomicBool>,
}

impl<S> TimedStream<S> {
    pub(super) fn new(source: S, idle: Option<Duration>, total: Option<Deadline>) -> Self {
        Self {
            inner: Some(Box::pin(source)),
            idle,
            idle_timer: None,
            total: total
                .map(|deadline| (deadline, Box::pin(tokio::time::sleep_until(deadline.at)))),
            reset_idle: Arc::new(AtomicBool::new(false)),
        }
    }

    fn finish(&mut self) {
        self.inner = None;
        self.idle_timer = None;
        self.total = None;
    }
}

impl<S: Stream<Item = Result<Bytes, Error>>> Stream for TimedStream<S> {
    type Item = Result<Bytes, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.inner.is_none() {
            return Poll::Ready(None);
        }
        if let Some((deadline, timer)) = &mut this.total {
            if timer.as_mut().poll(cx).is_ready() {
                let error = deadline.error();
                this.finish();
                return Poll::Ready(Some(Err(error)));
            }
        }
        // The batcher may have a pending source read when it flushes a batch.
        // Start a fresh idle budget after downstream backpressure, rather than
        // charging the caller's batch processing time to the remote service.
        if this.reset_idle.swap(false, Ordering::Relaxed) {
            this.idle_timer = None;
        }
        if let Some(duration) = this.idle {
            let timer = this
                .idle_timer
                .get_or_insert_with(|| Box::pin(tokio::time::sleep(duration)));
            if timer.as_mut().poll(cx).is_ready() {
                this.finish();
                return Poll::Ready(Some(Err(Error::Timeout {
                    phase: TimeoutPhase::BodyIdle,
                    duration,
                })));
            }
        }
        let result = this.inner.as_mut().unwrap().as_mut().poll_next(cx);
        // Synchronous response verification can run past an absolute deadline.
        if let Some((deadline, _)) = this
            .total
            .as_ref()
            .filter(|(deadline, _)| Instant::now() >= deadline.at)
        {
            let error = deadline.error();
            this.finish();
            return Poll::Ready(Some(Err(error)));
        }
        match &result {
            Poll::Ready(Some(Ok(bytes))) if !bytes.is_empty() => this.idle_timer = None,
            Poll::Ready(None | Some(Err(_))) => this.finish(),
            _ => {},
        }
        result
    }
}

pub(super) fn batches(
    source: impl Stream<Item = Result<Bytes, Error>> + Send + 'static,
    batching: BatchOptions,
    idle: Option<Duration>,
    total: Option<Deadline>,
) -> JsonlStream {
    let source = TimedStream::new(source, idle, total);
    let reset_idle = source.reset_idle.clone();
    Box::pin(Batches {
        inner: jsonl_batches_from_chunks(source, batching),
        reset_idle,
    })
}

struct Batches {
    inner: JsonlStream,
    reset_idle: Arc<AtomicBool>,
}

impl Stream for Batches {
    type Item = Result<super::JsonlBatch, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let result = self.inner.as_mut().poll_next(cx);
        if result.is_ready() {
            self.reset_idle.store(true, Ordering::Relaxed);
        }
        result
    }
}
