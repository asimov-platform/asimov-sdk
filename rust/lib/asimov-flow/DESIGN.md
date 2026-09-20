# First steps toward flow-based ASIMOV components

## Agreed boundaries

- `asimov-patterns` describes reusable component roles and logical input/output
  contracts. `Execute<T>` runs a configured operation; it is independent of where
  that operation executes.
- `asimov-flow` owns shared data-flow payloads and adapters. JSONL lines and
  batches now live here, with generic stream errors. It is also the future home
  for transport-independent pipeline/graph composition.
- `asimov-runner` owns local execution: subprocesses today, in-process component
  execution later. Its existing public line/batch paths re-export the flow types.
- `asimov-remote` owns remote execution. The `http` feature/module uses rustls,
  HTTP/2 negotiation, and HTTP/1.1 streaming fallback. The `iroh` feature/module
  is an empty stub for the future Iroh/QUIC transport.
- `asimov-social` keeps its domain request API and delegates HTTP transport to
  `asimov-remote`, returning raw JSONL batches.

Dependencies point from the executors to flow and patterns, never from flow to
an executor or between local and remote executors. Each backend retains its own
error type. Cross-backend connections must map errors explicitly.

For example, an HTTP batch stream can already supply an existing local graph
consumer through `GraphInput::Jsonl`, without parsing or reserializing it:

```rust,ignore
use asimov_flow::StreamExt;
use asimov_runner::GraphInput;

let input = GraphInput::Jsonl(Box::pin(remote_batches.map(|batch| {
    batch.map_err(|error| std::io::Error::other(error).into())
})));
```

This is an explicit local input adapter, not a new global error taxonomy or a
transport-independent pipeline supervisor.

## What this slice implements

The shared transport preserves bytes, blank lines, LF/CRLF, and an unterminated
final record. It does not validate JSON, UTF-8, RDF, or logical entry boundaries.
The pull-based stream supplies backpressure. Count, target bytes, and collection
delay bound batching; individual records can exceed the target byte size.
Complete buffered lines precede a terminal source error. A partial line is not
emitted if the source fails. Both executors use these same payload types.

Batch types, stream aliases, chunk-to-line framing, and batch flattening are
runtime-independent. Only reader adapters and timed batch accumulation require
the `tokio` feature; vectored wire slices require `std`.

Remote `http::Fetcher` and `http::Lister` are configured operations implementing
the existing pattern traits and `Execute<BatchStream<http::Error>>`. Their wire
profile is explicit: JSON POSTs to `fetch` and `list`, returning `application/jsonl`.
Fetcher input remains one resource URL; the SocialClient façade additionally
supports the service's multi-URL request. Raw command-line `other` arguments and
unsupported sort/cursor/output options are rejected before execution.

Remote listing delegates offset and limit to the endpoint as entry counts; it
does not silently treat records as entries. The existing local lister also has
a protective *line* cap. A shared entry-aware limiting policy requires agreement
on an RDF mapping/entry envelope. Zero-limit remote pattern invocations validate
options and then return an empty stream without a request.

Successful `execute()` means startup succeeded. Streams report subsequent body
or subprocess failures. Exhausting an HTTP body establishes transport completion,
not an application-level success protocol. Dropping its stream releases the
response; it cannot promise cancellation of remote work.

## async-flow review (0.1.5)

Reviewed the published documentation and the tagged sources:

- <https://docs.rs/async-flow/0.1.5/async_flow/>
- <https://github.com/artob/async-flow/tree/0.1.5/src/model>
- <https://github.com/artob/async-flow/blob/0.1.5/src/tokio/system.rs>
- <https://github.com/artob/async-flow/blob/0.1.5/src/tokio/scheduler/serial.rs>

The model has typed `Inputs<T>` / `Outputs<T>`, cardinality parameters, opaque
port IDs, `BlockDefinition`, and a typed `SystemBuilder::connect<T>`. Runtime
ports provide asynchronous send/receive backed by bounded channels. These are
useful foundations for packets whose payload is a `JsonlBatch`.

Important gaps before using it as ASIMOV's supervisor:

1. `Scheduler` and `Connection<T>` are currently empty traits. Block definitions
   enumerate port IDs, but there is no complete definition-to-execution binding.
2. Tokio `System::execute()` awaits `JoinSet::join_all()` but discards the returned
   block results and returns `Ok(())`. Ordinary block errors therefore disappear;
   task panics/cancellation are governed by `join_all`, not a typed system outcome.
   `SerialScheduler` similarly discards task handles/results.
3. The serial scheduler's `create<T>` constrains input and output to the same
   payload type. Heterogeneous components need independent input/output types or
   a typed port bundle.
4. Model connection metadata in 0.1.5 records port-ID pairs, not a distributed
   payload schema. Rust `TypeId`/type names alone cannot identify interoperable
   wire schemas across independently built remote services.

Suggested upstream work: separate immutable component schemas, configured
instances, and running activations; retain task identities/results; define EOF,
failure, cancellation, and channel-closure semantics; support independent port
types; preserve cardinality and explicitly define fan-in/fan-out behavior. Keep
the bounded-port interface compatible with pull-stream adapters rather than
requiring a task and queue at every edge.

## Decisions for the next slice

- **Typed schemas:** how should the pattern's URL/configuration inputs and graph
  output be represented as reusable async-flow port definitions? Separate logical
  options from CLI-only arguments, and define whether URLs are initialization
  packets or a continuing input stream. Marker traits alone are not port schemas.
- **Lifecycle:** keep `Execute<T>` as activation, and decide whether an activation
  needs a separate completion/cancellation handle in addition to its result stream.
  A scheduler must not treat successful startup as successful component completion.
- **Graph identity and failures:** design backend-neutral component/stage identity
  and explicit error adaptation for heterogeneous graphs, without adding remote
  errors to the local runner error enum.
- **Pipeline ownership:** define a transport-neutral pipeline in `asimov-flow`
  that can later be a DAG. The current sealed, process-specific runner pipeline
  remains a local optimization; its native OS-pipe edges and child supervision
  should become backend capabilities, not the generalized graph model.
- **Wire protocol:** confirm common request schemas, RDF mapping profiles, entry
  boundaries, and terminal remote outcomes before extending fetch/list or adding
  Iroh. HTTP/2 DATA-frame boundaries and local batch boundaries are not records.

This slice establishes a tested payload/transport boundary without introducing a
second FBP scheduler or committing to a generalized DAG API.
