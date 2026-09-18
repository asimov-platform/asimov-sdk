// This is free and unencumbered software released into the public domain.

//! The asynchronous execution interface shared by all program patterns.
//!
//! [`Execute`] separates running an already configured operation from selecting
//! its role, input, and options. Pattern traits in [`crate::programs`] extend
//! this interface without adding methods or prescribing an executor.

use alloc::boxed::Box;
use async_trait::async_trait;
use core::result::Result;

/// An asynchronously executable operation yielding `T` or its associated [`Error`](Self::Error).
///
/// Inputs and configuration are supplied by the implementation, typically at
/// construction time. The mutable receiver permits execution to advance input
/// streams or update internal state; it does not promise that another call will
/// replay the same input. Implementations should document reuse, buffering,
/// cancellation, and the meaning of successful completion.
/// If `T` is a live stream, `Ok(T)` can mean successful startup rather than
/// completed execution; subsequent failures must be exposed by that result.
/// Each implementation chooses one error type for its result type `T`. Callers
/// can constrain it with `Execute<T, Error = E>` or use the corresponding
/// pattern trait's inherited associated type, such as `Emitter<T, Error = E>`.
///
/// This trait uses [`#[async_trait]`](macro@async_trait) with `Send` futures.
/// Implementors use the same attribute on their `impl`; each call returns a
/// boxed future whose lifetime is tied to the mutable receiver. The trait
/// itself does not require `Send` or `Sync` as supertraits, select an
/// asynchronous runtime, or require an operating-system process.
///
/// # Example
///
/// An in-process emitter can use the same interface as a process-backed one:
///
/// ```
/// use asimov_patterns::{Emitter, Execute};
/// use async_trait::async_trait;
/// use core::convert::Infallible;
///
/// struct EmptyEmitter;
///
/// #[async_trait]
/// impl Execute<Vec<u8>> for EmptyEmitter {
///     type Error = Infallible;
///
///     async fn execute(&mut self) -> Result<Vec<u8>, Self::Error> {
///         // An empty N-Triples document represents an empty RDF graph.
///         Ok(Vec::new())
///     }
/// }
///
/// impl Emitter<Vec<u8>> for EmptyEmitter {}
///
/// // Associated errors can be constrained on generic bounds and trait objects.
/// async fn emit(
///     emitter: &mut (impl Emitter<Vec<u8>, Error = Infallible> + ?Sized),
/// ) -> Result<Vec<u8>, Infallible> {
///     emitter.execute().await
/// }
///
/// let mut emitter = EmptyEmitter;
/// let emitter: &mut dyn Emitter<Vec<u8>, Error = Infallible> = &mut emitter;
/// let operation = emit(emitter);
/// ```
#[async_trait]
pub trait Execute<T> {
    /// The implementation-specific error returned directly by execution.
    ///
    /// This need not be a subprocess error or implement `core::error::Error`.
    /// Errors carried inside a streaming result `T` are specified separately by
    /// that result's type and documented by the implementation.
    type Error;

    /// Executes the operation using its current input and configuration.
    ///
    /// For a process-backed implementation reporting completed results, success
    /// requires a normal zero exit status and successful required I/O transfers.
    /// A streaming implementation must also expose eventual completion or
    /// failure; receiving some output is not proof of success. An error does
    /// not imply that external side effects have been rolled back.
    ///
    /// # Errors
    ///
    /// Returns the implementation-defined [`Error`](Self::Error). Implementations document
    /// which failures are returned directly and which are delivered through `T`,
    /// including launch, transport, program, and decoding failures where applicable.
    async fn execute(&mut self) -> Result<T, Self::Error>;
}
