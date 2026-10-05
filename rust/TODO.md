# Rust workspace backlog

Review date: 2026-10-04. Scope: all 41 workspace members, their feature and
dependency wiring, public APIs, implementations, tests, and build/documentation
configuration. Paths below are relative to `rust/` unless prefixed with `../`.

P1 items address data integrity, destructive failure paths, and panics. P2 items
address correctness, build combinations, and operational reliability. P3 items
are follow-up design and maintenance work. Each checkbox is open work; runtime
reproductions and build results are distinguished from source-review findings.

## P1: Data integrity and failure handling

- [ ] **Validate downloaded manifest paths and module identity before use.**
  In `lib/asimov-installer/src/installer.rs`, `assemble_module` joins unchecked
  `provides.programs` strings onto extraction and installation directories, then
  renames files and changes permissions. Reject absolute paths, traversal,
  separators, and unexpected file types before mutation. `preinstall`
  must also require the manifest's name to equal the requested `ModuleName`.
  Cover malformed manifests and symlinked binaries with local fixtures.

- [ ] **Align manifest readers with the installed directory layout.**
  `ModuleManifest::read_manifest` in
  `lib/asimov-module/src/models/module_manifest.rs` searches legacy flat files,
  while `Registry` writes `modules/installed/<name>/manifest.json`.
  `Resolver::try_from_dir` in `lib/asimov-module/src/resolve.rs` also reads only
  flat YAML files. Share the layout contract and test lookup/resolution against
  a registry-created installation, retaining explicit legacy compatibility.

- [ ] **Make upgrades recoverable before removing the working installation.**
  `Installer::upgrade_module` in `lib/asimov-installer/src/installer.rs`
  uninstalls the old version before assembly, registration, and re-enabling
  can succeed. Assemble and validate the replacement first, retain a rollback
  copy, and commit directory/link changes together. Test missing binaries,
  failed renames, link failures, and interrupted upgrades with the prior
  version still usable.

- [ ] **Preserve key identity when the public-key cache is missing or stale.**
  In `lib/asimov-keyring/src/keyring.rs`, `my_public_key` calls `rekey` when the
  public file is absent, even if the secret still exists; `ensure_secret_key`
  does not repair the public file. Derive and atomically repair the public key
  from the existing secret. Make failed public-file writes during explicit rekey
  recoverable and test the missing, stale, and unwritable cache cases with a
  mock store.

- [ ] **Check credit range and precision at conversion boundaries.**
  `Credits::as_nanos` in `lib/asimov-credit/src/credits.rs` narrows an `i128`
  mantissa with `as`: ten billion credits becomes `-8446744073709551616` nanos
  (reproduced). Rescaling can also fail its assertion, while Serde silently
  rounds `0.0000000001` to `"0.000000000"` (reproduced). Define the supported
  range/precision, provide checked conversion/arithmetic, and test boundary
  values and exact serialization round trips.

- [ ] **Preserve snapshot identity and subsecond timestamps.**
  `lib/asimov-snapshot/src/storage/fs.rs` stores timestamps only to seconds.
  Subsecond round trips lose information, a second snapshot in the same second
  fails with `DirectoryNotEmpty`, and deleting with the original timestamp can
  leave `current` pointing to the removed snapshot (all reproduced). Use a
  collision-safe, precision-preserving representation with a legacy reader;
  test multiple same-second saves and deletion through both timestamp forms.

- [ ] **Respect the requested Hugging Face revision throughout a snapshot.**
  `ensure_snapshot` in `lib/asimov-huggingface/src/ensure.rs` queries/downloads
  the requested revision but checks cached files using a default-revision
  `Repo`. This can mix revisions. It also returns the first file's parent, which
  can be a nested directory rather than the snapshot root. Resolve one commit,
  use it consistently for all files, and test two cached revisions plus a
  repository whose first listed file is nested.

## P2: Build, feature, and facade correctness

- [ ] **Repair the Nexus validator integration and dimensional contract.**
  `lib/asimov-nexus/src/search_request.rs` fails to compile with `validator`:
  the derive mishandles the default const generic and integer range literals
  are used for `f32`. Cloud's all-features build also fails (reproduced).
  Make validation compile, enforce the dimension represented by `N` rather than
  a hardcoded minimum of 128, and reject non-finite vector/score values. Test
  alternate dimensions, wrong lengths, score bounds, and NaN/infinity.

- [ ] **Resolve the workspace-wide Iroh integration feature conflict.**
  `cargo check --workspace --all-features --all-targets --locked` fails in
  `iroh-blobs 0.103.0`: its match omits `irpc::Error::Write` with `irpc 0.17.0`.
  The isolated `asimov-kb --all-features` library check passes, so retain a
  whole-workspace integration check when selecting an upstream fix or compatible
  dependency versions in `Cargo.toml`.

- [ ] **Make the no-std contract hold on a target without std.**
  Host `--no-default-features` checks pass, but `asimov-core` fails for
  `thumbv7em-none-eabihf`: default-enabled `know` pulls `serde_json/std` and
  `memchr/std` (reproduced and traced with `cargo tree`). Audit dependency
  defaults from `Cargo.toml` and `lib/asimov-core/Cargo.toml` outward. Restore
  `#![no_std]` in `asimov-env` and `asimov-server`, and gate OS/runtime-only
  dependencies, modules, and re-exports together. Add a genuine no-std target
  check alongside host checks.

- [ ] **Normalize dependency defaults and shared dependency inheritance.**
  Replace explicitly re-enabled dependency defaults in runner, installer,
  snapshot, and server with required features, and inherit shared
  dependencies consistently to make each crate's build surface predictable.

- [ ] **Complete and test SDK facade wiring.**
  The `serde` flag in `lib/asimov-sdk/Cargo.toml` reaches only core. Define the
  intended facade-level integration bundles, wire optional dependencies
  explicitly, and compile consumer examples for each advertised feature
  independently.

- [ ] **Exercise workspace members and optional integrations in CI.**
  `../.github/workflows/ci.yaml` currently passes
  `build_whole_workspace: false`. Add explicit coverage for all members,
  independent default/no-default/std builds, and optional integrations,
  including the failures above. Preserve the synchronized Rust 1.97.1 MSRV and
  add Windows and applicable WebAssembly/no-std smoke checks; host feature
  unification is insufficient to validate these contracts.

## P2: Modules, environment, and persistent state

- [ ] **Detect dependency cycles and reuse the chosen release during install.**
  `preinstall` in `lib/asimov-installer/src/installer.rs` recursively installs
  dependencies without an in-progress set, so self/cyclic dependencies recurse
  indefinitely. `upgrade_module` also resolves latest once for comparison and
  again inside preinstall. Track the dependency chain and pin one selected
  release through the operation; test cycles, diamond dependencies, and a
  changing latest-release response.

- [ ] **Validate GitHub release redirects and cover asset fallback.**
  In `lib/asimov-installer/src/installer/github.rs`, redirect lookup accepts the
  final path segment without validating status or tag-route shape. Test failed
  redirects and asset fallback using injectable local release endpoints.

- [ ] **Make executable registration ownership-aware and recoverable.**
  `Registry::add_module` in `lib/asimov-registry/src/registry.rs` commits the
  module directory before linking binaries, and `add_binary` removes an existing
  destination without checking its owner. Uninstall in installer removes names
  from the manifest regardless of their current targets. Preflight collisions,
  validate executable entries, and roll back failed publication; test two
  modules providing the same name and partial link failures.

- [ ] **Make legacy registry migration atomic and observable.**
  `move_legacy_manifest` in `lib/asimov-registry/src/registry.rs` writes
  directly to the final manifest and treats an existing destination as reason
  to delete the legacy copy. A partial destination can therefore destroy the
  recoverable source on a later attempt. Validate before replacement, write via
  a temporary file, propagate migration/I/O failures, and serialize concurrent
  migration/install operations. Include interrupted-write recovery tests.

- [ ] **Unify state-root selection across configuration and storage APIs.**
  `lib/asimov-env/src/paths.rs` honors `ASIMOV_ROOT` and platform defaults,
  `lib/asimov-directory/src/fs/state_directory.rs` always uses `~/.asimov`, and
  core/env `config_dir` separately use `ASIMOV_HOME` and `~/.config/asimov`.
  Define their intended relationship, share root resolution, and propagate
  missing-home errors instead of panicking. Test custom roots, absent home
  variables, and Windows paths, including keyring and peer resolution.

- [ ] **Constrain configuration and public-key filename components.**
  `ModuleManifest::variable` joins unchecked profile/module/variable names in
  `lib/asimov-module/src/models/module_manifest.rs`; keyring `get_public_key`
  and `rekey` join arbitrary users in `lib/asimov-keyring/src/keyring.rs`.
  `read_manifest` also accepts a raw module-name path. Validate components
  before reads/writes and test absolute paths, parent traversal, platform
  separators, and symlink escape behavior.

- [ ] **Give keyring handles a coordinated store lifetime.**
  `lib/asimov-keyring/src/keyring.rs::open` replaces the process-global default
  store, and `close` unsets it for every handle. An early `my_public_key` error
  skips close entirely. Introduce explicit backend ownership/injection and a
  shared lifetime guard; test overlapping handles, concurrent access, and
  cleanup after errors without touching the operating system's real keyring.

- [ ] **Honor optional configuration variables when reading a profile.**
  `read_variables` in `lib/asimov-module/src/models/module_manifest.rs` calls
  `variable` for every declaration, including optional variables with no value,
  and fails the entire collection. Omit absent optional values while preserving
  errors for required values and unreadable files. Test environment overrides,
  configured files, defaults, and absent optional/required variables together.

- [ ] **Make directory/configuration interfaces report real state and errors.**
  `lib/asimov-directory/src/fs/module_directory.rs` returns `false` for both
  installation/enabling checks; `config_profile.rs` always returns no prompt.
  Implement these contracts against the shared layout. Change the iterators in
  `fs/module_iterators.rs` to expose directory/read/parse failures rather than
  silently dropping entries; validate that `open` paths are directories. Cover
  malformed manifests, broken links, and inaccessible entries.

- [ ] **Fix Python environment discovery and initialization outcomes.**
  `lib/asimov-env/src/envs/python.rs::lib_path` appends each fallback version
  onto the previous candidate, so versions below 3.13 are searched under nested
  nonexistent paths. Discover paths from the selected interpreter, including
  Windows and system environments. `initialize` ignores a failed venv command
  and leaves a directory that `is_initialized` accepts. Check interpreter/venv
  viability and child status; exercise failure and retry with local fixtures.

- [ ] **Make environment module queries truthful and reap Cargo subprocesses.**
  `lib/asimov-env/src/env.rs` defaults enabled checks to `true` and available
  modules to an empty list; language adapters retain these placeholders.
  Implement supported queries and expose unsupported operations explicitly.
  `envs/cargo.rs::installed_modules` drops the child handle and discards stderr
  without checking exit status. Retain/wait for the child and distinguish
  command failure from an empty inventory; test Ruby/system discovery too.

- [ ] **Make multi-file authoring edits recoverable.**
  `lib/asimov-module-kit/src/module/program.rs::add_program` writes the source
  and Cargo manifest before updating `.asimov/module.yaml`; an error leaves a
  partially applied operation that cannot simply be retried. Validate/render
  all edits first and use staged writes with rollback. Cover a malformed or
  unwritable manifest, a failed Cargo update, and interrupted creation.

- [ ] **Match Cargo target discovery in module linting.**
  `lib/asimov-module-kit/src/module/lint.rs` considers only explicit `[[bin]]`
  declarations, so valid auto-discovered `src/bin` targets can be reported as
  absent/orphaned. It also does not diagnose a missing declared source file.
  Account for package-name `src/main.rs`, automatic binaries, `autobins`, and
  explicit paths; propagate directory-scan errors instead of flattening them.

- [ ] **Make snapshot publication, current selection, and deletion consistent.**
  In `lib/asimov-snapshot/src/storage/fs.rs`, publication creates the final
  directory before rename, current-link replacement removes the old link first,
  and Windows uses a file symlink for a directory target. Stage publication and
  current updates atomically, coordinate concurrent writers, and clean up the
  URL inventory when its last snapshot disappears. Test races, failed saves,
  empty histories, and Windows directory-link behavior.

- [ ] **Refresh snapshot resolution and support injectable execution/storage.**
  `lib/asimov-snapshot/src/snapshot.rs` caches module resolution indefinitely,
  selects only the first matching module, and resolves executables through the
  global runner root despite accepting a custom registry. Add invalidation and
  registry-aware execution. Handle a missing current snapshot as an explicit
  cache-miss policy, and use fixtures to test freshness, module changes,
  candidate failures, and retention rules.

- [ ] **Remove blocking work from asynchronous execution paths.**
  Installer preinstall calls synchronous Hugging Face downloads directly, and
  Snapshotter calls synchronous storage methods from async methods. MCP's
  `lib/asimov-server/src/http/mcp/server.rs` invokes synchronous user callbacks
  directly on the runtime. Provide async adapters or bounded blocking workers
  with cancellation semantics; verify an unrelated task remains responsive
  during a slow download, storage operation, or tool callback.

## P2: Protocols, execution, and service APIs

- [ ] **Bound proxy connection, upload, and shutdown lifetimes.**
  `lib/asimov-proxy/src/openai.rs` needs upload deadlines and bounded concurrency;
  stalled response streams can indefinitely delay graceful shutdown. Add
  per-stage deadlines for CONNECT, SOCKS, and TLS in `proxy_connector.rs`, and
  test client disconnection and cancellation without buffering responses.

- [ ] **Complete upstream-proxy interoperability coverage.**
  `lib/asimov-proxy/src/proxy_config.rs` lacks port-aware NO_PROXY rules.
  `proxy_connector.rs` uses only the first locally resolved SOCKS address;
  CONNECT parsing must enforce its header cap even when the terminator arrives
  in the same read and preserve any following tunnel bytes. Test malformed
  CONNECT responses, HTTPS proxies, SOCKS DNS modes, and multi-address fallback
  against local fixtures.

- [ ] **Move proxy body logging off the response polling path.**
  `lib/asimov-proxy/src/body_logger.rs` locks a mutex and writes synchronously
  for each frame, discards write failures, and lacks exchange correlation IDs.
  Use a bounded writer queue with explicit failure/backpressure behavior and
  shutdown flushing. Test slow/full sinks while preserving streaming.

- [ ] **Negotiate peer versions/features and bound connection lifecycle waits.**
  `lib/asimov-protocol/src/peer_accept.rs` echoes the remote hello instead of
  advertising local capabilities, and neither handshake validates compatible
  version ranges/required features. Implement negotiation with typed failures,
  handshake/ping/close deadlines, and cancellation. Use local peer tests for
  incompatible versions, missing required features, and a silent peer.

- [ ] **Return explicit unsupported responses for unfinished HTTP endpoints.**
  Audio/image/embedding/chat/model handlers under
  `lib/asimov-server/src/http/openai_v1/` return dummy success data.
  GraphQL, SPARQL, and well-known handlers also contain placeholders.
  Implement an endpoint or return a protocol-appropriate unsupported/not-found
  error. Add router-level tests that exercise every mounted route and reject
  successful-looking fabricated results.

- [ ] **Make HTTP metrics router construction repeatable.**
  Constructing `http::routes()` twice in one process panics because the
  Prometheus router installs a global recorder each time (reproduced in tests).
  Separate recorder initialization from routing and test multiple routers.

- [ ] **Forward supported completion options and implement actual streaming.**
  Chat/completion handlers under `lib/asimov-server/src/http/openai_v1/` echo
  the requested model but construct `PrompterOptions::default()`, so the
  provider never receives it. Other generation options are silently ignored, and
  `chat/streaming.rs` buffers the whole provider response before starting SSE.
  Forward supported options, reject unsupported ones, and stream incremental
  output with cancellation/error reporting. Test model selection and first-byte
  delivery before process exit using a deterministic provider fixture.

- [ ] **Preserve independent prompt boundaries.**
  In `lib/asimov-prompt/src/prompt.rs`, OpenAI text-array prompts are joined
  without boundaries. Explicitly support or reject batched
  prompts. Define a structured chat input contract with runner rather than
  relying on unescaped `role: text` display strings for all input formats.
  OpenAI system content supplied as text-part arrays currently returns `Err(())`
  from the borrowed conversion; define and test multipart text handling too.

- [ ] **Honor HTTP protocol feature gates and transport configuration.**
  `lib/asimov-server/src/http.rs` always mounts GraphQL/GSP/SPARQL/MCP routes
  regardless of their feature flags. The `https` feature only enables `http`,
  while `start` serves a plain TCP listener. Make enabled protocols reflect the
  actual router/transport and accept configured providers instead of always
  constructing an empty MCP server. Test feature-specific route availability.

- [ ] **Preserve protocol-specific error envelopes and status semantics.**
  `lib/asimov-server/src/http/mcp.rs` maps provider errors to bare HTTP 500
  responses and hardcodes the initialization version. Return JSON-RPC errors
  with request IDs and negotiate the supported version. OpenAI
  `http/openai_v1/error.rs` maps provider execution failures to HTTP 400 and
  emits a bare error object; map server/client errors separately and use the
  expected error envelope. Add failure-path interoperability fixtures.

- [ ] **Make server persistence durable, configurable, and recoverable.**
  `lib/asimov-server/src/persistence.rs` writes beside the executable, truncates
  the live file, and does not explicitly flush its `BufWriter`. It suppresses
  load errors and mutates in-memory state before a write can fail. Inject the
  state path, publish a flushed temporary file atomically, and preserve prior
  memory/disk state on failure. Test corrupt files, unwritable directories, and
  concurrent updates without a process-global singleton.

- [ ] **Offer bounded transport resources and configurable HTTP clients.**
  `lib/asimov-flow/src/jsonl.rs` has no line-size ceiling; runner buffers stderr
  and byte-oriented captured output without bounds. Add opt-in limits that
  preserve framing and terminal-error ordering. `lib/asimov-remote/src/http.rs`
  owns a hardcoded client with no connect/read timeout configuration; expose
  client/transport policy independently of the server-side `TimingOptions`.
  Apply reusable client/timeout configuration to cloud account balance calls.

- [ ] **Align SocialClient request options with the shared remote protocol.**
  `lib/asimov-social/src/fetch_request.rs` has an empty options struct and
  `list_request.rs` supports only offset/limit, while remote supports caching,
  filtering, timing, sort, cursors, and output options. Unsupported incoming
  fields are silently discarded on deserialize/reserialize. Share or explicitly
  map the wire option types, preserve explicit values, and add request parity
  tests for SocialClient and remote fetch/list operations.

- [ ] **Make remaining identifier/key conversions validate their invariants.**
  `lib/asimov-id/src/public_key.rs` pads/truncates `Vec<u8>` inputs and unwraps
  conversion to Iroh keys. Generic KB IDs also pad/truncate vectors.
  `lib/asimov-keyring/src/keyring.rs` panics on non-32-byte secrets before
  zeroizing the source buffer. Add checked byte/key conversions and typed
  corruption errors, keeping temporary secrets zeroized on every exit path.

- [ ] **Correct KB GraphQL scalar registration and validation.**
  Replace GraphQL names such as `PERSON ID`/`ID<16>` in `lib/asimov-kb/src/`
  with valid, consistently registered scalars whose validation checks the
  actual ID format.

- [ ] **Bound telemetry flush memory and checkpoint successful batches.**
  `lib/asimov-telemetry/src/client.rs::flush` holds every inactive log open and
  collects all events before sending 500-event batches. A later batch failure
  retains all files, replaying already accepted events on retry. Process a
  bounded number of logs/events and retain per-batch progress or stable event
  identity. Test a large backlog, partial HTTP failure, malformed/truncated
  logs, and file-descriptor limits with the existing local receiver fixture.

- [ ] **Coordinate telemetry initialization and runtime disable behavior.**
  `lib/asimov-telemetry/src/client.rs::start` checks existence then overwrites
  the installation ID, allowing concurrent first invocations to choose different
  IDs. `new` checks opt-out once, while existing clients and flush workers do
  not observe a later disable; deletion also races locked/open logs. Serialize
  initialization and define live-client/flush cancellation behavior. Test
  concurrent first starts and disabling while another invocation is active.

## P3: Design, coverage, and maintenance

- [ ] **Resolve shared execution semantics before generalizing pipelines.**
  Follow `lib/asimov-flow/DESIGN.md`: define logical entry boundaries separately
  from JSONL lines, typed port schemas, completion/cancellation outcomes, and
  explicit cross-backend error mapping. Local lister line caps and remote entry
  limits currently differ. Scope backend-neutral composition and
  `lib/asimov-remote/src/iroh.rs` implementation as small follow-up slices, each
  with lifecycle/backpressure conformance tests.

- [ ] **Replace non-hermetic and empty test cases with useful fixtures.**
  Several `lib/asimov-runner/src/programs/*::test_execute` bodies contain only
  TODOs. Use local process fixtures for uncovered invocation
  contracts, and retain network tests as explicit smoke tests. Add randomized
  framing/URL/ID round-trip tests and targeted failure injection for the P1
  findings rather than duplicating existing transport tests.

- [ ] **Make discovery configuration reflect the running server.**
  `lib/asimov-server/src/mdns.rs` fixes the instance name, UDP service type, and
  port 1920, and unwraps local-IP discovery. Accept the actual transport,
  listener address/port, and instance identity; return discovery errors and
  expose a managed daemon lifetime. Test advertisement data without depending
  on multicast availability.

- [ ] **Describe scaffold maturity and give integrations bounded next steps.**
  The `src/lib.rs` files for platform, repository, runtime, token, universe, and
  vault contain only crate setup. Add concise crate-level status/scope docs before
  defining each first API. Ledger currently exposes a hidden ERC-20 binding,
  and credit's database integration features only enable dependencies; specify
  useful, testable integration contracts for those surfaces rather than implying
  completed implementations.

- [ ] **Repair README generation dependencies and failure handling.**
  `Makefile` references the missing
  `../.config/readmer/rust/README.md.liquid`. Per-crate `Rakefile` tasks depend
  only on the shared Jinja template, omitting manifests, local examples, and the
  package table; they truncate output and ignore renderer exit status. Restore
  the workspace template, track all inputs, check subprocess status, and replace
  generated files atomically so a failed renderer preserves the prior document.

- [ ] **Refresh generated package metadata and valid feature examples.**
  `Cargo.toml`, `Rakefile`, and the shared template
  `../.config/readmer/rust/README.md.j2` link to the old `asimov.rs` repository
  layout. The shared template advertises
  `features = ["tracing"]` even for remote and telemetry, which do not export
  that feature, and describes workspace capabilities on empty scaffold crates.
  Generate accurate per-crate examples from metadata, use the actual Rust
  1.97.1 minimum, and refresh package tables/READMEs through their generators.

- [ ] **Enforce warning-free rustdoc and document public failure contracts.**
  Make warning-free documentation builds a CI check. Prioritize missing
  error, persistence, and feature-availability documentation on the public
  registry, installer, protocol, credit, and snapshot APIs.

## Validation evidence

Checks ran on macOS/aarch64 with Rust 1.98.1, plus a Rust 1.97.1 MSRV build.
Debug information/incremental compilation were disabled for selected builds to
limit disk use. Findings above remain open despite passing default tests.

- Default workspace all-target checks passed with both toolchains:
  `cargo check --workspace --all-targets --locked` and
  `cargo +1.97.1 check --workspace --all-targets --locked`.
- Each of the 41 members passed isolated default and
  `--lib --no-default-features` checks. Isolated std-only checks failed only for
  installer and registry; additional module-kit `module`/`lint` checks failed.
- Workspace tests, including doctests, passed using:

  ```sh
  cargo test --workspace --locked --no-fail-fast -- \
    --skip executor::tests::test_success
  ```

  The skipped test uses live Google access;
  Hugging Face and module-template network smoke tests remained ignored.
  `cargo test -p asimov-patterns --all-features --locked` also passed.
- Isolated all-features library checks passed for credit, id, kb, keyring,
  module-kit, server, and social; cloud and nexus failed in validator
  derivation. The workspace all-features/all-targets check stopped at the Iroh
  conflict.
- The genuine no-std check failed through `know`'s std features:

  ```sh
  cargo check -p asimov-core --lib --no-default-features \
    --target thumbv7em-none-eabihf --locked
  ```

- Documentation for all 41 members built with the warnings listed above.
- Temporary local probes reproduced manifest field loss/model round-trip
  failure, wrong-class ID deserialization, blob length metadata, credit
  overflow/rounding, system-role conversion, empty peer resolution/oversized
  frame panics, duplicate/extension resolver matches, and snapshot timestamp,
  collision, and dangling-current behavior. These cases should become focused
  regression tests when their fixes are implemented.
