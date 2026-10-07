# Rust workspace backlog

Review date: 2026-10-07, against `a121139`. Scope: all 41 workspace members,
including the documented scaffolds; feature/dependency wiring, public APIs,
implementations, tests, and build/documentation configuration. Paths below are
relative to `rust/` unless prefixed with `../`.

P1 items address data integrity, destructive failure paths, and panics. P2 items
address correctness, build combinations, and operational reliability. P3 items
are follow-up design and maintenance work. Each checkbox is open work; runtime
reproductions and build results are distinguished from source-review findings.
Unless marked reproduced, implementation findings are based on source review.

## P1: Data integrity and failure handling

- [ ] **Validate downloaded manifest paths and module identity before use.**
  In `lib/asimov-installer/src/installer.rs`, `assemble_module` joins unchecked
  `provides.programs` strings onto extraction and installation directories, then
  renames files and changes permissions. Reject absolute paths, traversal,
  separators, and unexpected file types before mutation. `preinstall`
  must also require the manifest's name to equal the requested `ModuleName`.
  Registry publication validation happens after assembly and cannot protect
  these earlier filesystem operations. Cover malformed manifests and symlinked
  binaries with local fixtures.

- [ ] **Make legacy registry migration atomic and observable.**
  `move_legacy_manifest` in `lib/asimov-registry/src/registry.rs` writes
  directly to the final manifest and deletes the legacy copy whenever the
  destination exists. A local fixture with a valid legacy manifest and a
  truncated destination reproduced loss of the recoverable source during
  `read_manifest`. Validate before replacement, write via a temporary file,
  propagate migration/I/O failures, and serialize concurrent migration/install
  operations. Include interrupted-write recovery tests.

- [ ] **Check credit range and precision at conversion boundaries.**
  `Credits::as_nanos` in `lib/asimov-credit/src/credits.rs` narrows an `i128`
  mantissa with `as`: ten billion credits becomes `-8446744073709551616` nanos
  (reproduced). Rescaling can also fail its assertion, while the string
  conversion used by Serde rounds `0.0000000001` to `"0.000000000"`
  (reproduced). Define the supported range/precision, provide checked
  conversion/arithmetic, and test boundary values and exact serialization
  round trips.

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

- [ ] **Make KB UUID creation features self-contained.**
  `lib/asimov-kb/src/id.rs::new_uuid` calls `Uuid::now_v7`, which requires
  `uuid/std`, but the `uuid`/`new` features do not require `std`. Reproduced:
  `cargo check -p asimov-kb --lib --no-default-features
  --features new --locked` reports
  E0599 for `now_v7`. Gate clock-based constructors with `std` or make their
  feature imply it; provide caller-supplied time/randomness for no-std creation.
  Cover the `uuid`, `new`, and `all` features without defaults independently.

- [ ] **Make the no-std contract hold on a target without std.**
  Host `--no-default-features` checks pass, but `asimov-core` fails for
  `thumbv7em-none-eabihf`: default-enabled `know` pulls `serde_json/std` and
  `memchr/std` (reproduced and traced with `cargo tree`). Audit dependency
  defaults from `Cargo.toml` and `lib/asimov-core/Cargo.toml` outward, including
  `know`'s pretty-printing dependencies. Restore `#![no_std]` in `asimov-env`,
  `asimov-huggingface`, and `asimov-server`. Gate OS/runtime-only dependencies,
  modules, and re-exports together. Add a genuine no-std target check alongside
  host checks.

- [ ] **Normalize dependency defaults and shared dependency inheritance.**
  Replace explicitly re-enabled dependency defaults in runner, installer,
  snapshot, and server with required features, and inherit shared
  dependencies consistently to make each crate's build surface predictable.
  Also audit default-enabled `derive_more` in cloud/credit/nexus and native
  keyring dependencies; disabling this crate's defaults does not disable theirs.

- [ ] **Complete and test SDK facade wiring.**
  The `serde` flag in `lib/asimov-sdk/Cargo.toml` reaches only core. Define the
  intended facade-level integration bundles, wire optional dependencies
  explicitly, and compile consumer examples for each advertised feature
  independently. For example, `std,credit,serde` does not enable Serde on
  `Credits`, and `std,id,serde` does not enable it on `PublicKey`.

- [ ] **Add isolated feature and cross-target coverage to CI.**
  `../.github/workflows/ci.yaml` passes `build_whole_workspace: false`, but bare
  Cargo commands at this virtual workspace root still select all members;
  current macOS/Linux/Windows jobs build and test them. Make that selection
  explicit and add independent default/no-default/std builds and optional
  integration combinations, including KB's creation features above. Include
  `cargo check --workspace --all-features --all-targets --locked` to catch
  cross-crate feature conflicts, including KB Iroh support with protocol test
  dependencies. Preserve the Rust 1.97.1 MSRV and native Windows fixtures; add
  applicable WebAssembly/no-std smoke checks. Host feature unification is
  insufficient to validate these contracts.

## P2: Modules, environment, and persistent state

- [ ] **Support GNOME and KDE desktop keyrings over D-Bus.**
  Add persistent GNOME Keyring and KDE KWallet support to `lib/asimov-keyring/`
  through their D-Bus interfaces.

- [ ] **Provide explicit identity migration between keyring backends.**
  `lib/asimov-keyring/` keeps native and file secrets separate. Add an explicit
  migration API that verifies the destination before switching, preserving the
  public identity. Cover Linux kernel-key expiry/reboot and existing caches
  when no secret is available; a public key alone cannot recover the identity.
  `lib/asimov-keyring/src/store.rs::platform_backend` selects a volatile mock
  outside Apple, Linux, and Windows. Use persistent storage or an explicit
  unsupported error there so reopening a default keyring does not silently
  create a new identity.

- [ ] **Detect dependency cycles during recursive installation.**
  `preinstall` in `lib/asimov-installer/src/installer.rs` recursively installs
  dependencies without an in-progress set, so self/cyclic dependencies recurse
  indefinitely. Track the dependency chain; test cycles and diamond
  dependencies.

- [ ] **Validate GitHub release redirects and cover asset fallback.**
  In `lib/asimov-installer/src/installer/github.rs`, redirect lookup accepts the
  final path segment without validating status or tag-route shape. Test failed
  redirects and asset fallback using injectable local release endpoints.

- [ ] **Coordinate low-level registry mutations and read-time recovery.**
  Extend publication locking to legacy migration, enable/disable, and low-level
  removal APIs. Recover interrupted publications before general registry reads.
  Publication currently has a brief reader-visible directory/link gap;
  assess generation-based publication and power-loss durability separately.

- [ ] **Unify state-root selection across configuration and storage APIs.**
  `lib/asimov-env/src/paths.rs` honors `ASIMOV_ROOT` and platform defaults,
  `lib/asimov-directory/src/fs/state_directory.rs` always uses `~/.asimov`, and
  core/env `config_dir` separately use `ASIMOV_HOME` and `~/.config/asimov`.
  Define their intended relationship, share root resolution, and propagate
  missing-home errors instead of panicking. Test custom roots, absent home
  variables, and Windows paths, including keyring and peer resolution.

- [ ] **Honor optional configuration variables when reading a profile.**
  `read_variables` in `lib/asimov-module/src/models/module_manifest.rs` calls
  `variable` for every declaration, including optional variables with no value,
  and fails the entire collection. Omit absent optional values while preserving
  errors for required values and unreadable files. Test environment overrides,
  configured files, defaults, and absent optional/required variables together.

- [ ] **Distinguish filesystem paths from IRIs before normalization.**
  `lib/asimov-module/src/normalization.rs::normalize_url` parses raw filenames
  as IRI references: existing files named `a#b.txt` and `a?b.txt` acquire a
  fragment/query instead of escaped filename characters. `C:\work\file.txt`
  errors before Windows-specific handling, while `C:/work/file.txt` becomes
  the `c:` scheme (all reproduced). Classify drive/UNC and local paths first,
  escape path bytes once, and preserve already encoded URL semantics. Cover
  reserved characters, spaces in parent directories, symlinks, and Windows
  paths; return a missing-home error from tilde expansion instead of panicking.

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

- [ ] **Preserve YAML sequence structure when adding programs.**
  `lib/asimov-module-kit/src/module/manifest_edit.rs::insert_program_line`
  derives list indentation from `programs:` rather than existing items. With
  a valid indentless sequence (`  programs:` followed by `  - old`), appending
  a program succeeds but reparses as one string, `old - new` (reproduced).
  Locate the actual sequence and its indentation, preserve comments, and
  validate the edited document before writing. Cover indentless/indented lists,
  intervening comments, nested keys, and rejection of unsupported YAML shapes.

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
  `lib/asimov-proxy/src/openai.rs` needs upload deadlines and bounded
  concurrency; stalled response streams can indefinitely delay shutdown. Add
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
  Audio/image/embedding, stored-chat, and model handlers under
  `lib/asimov-server/src/http/openai_v1/` return dummy success data.
  GraphQL, SPARQL, and well-known handlers also contain placeholders.
  Implement an endpoint or return a protocol-appropriate unsupported/not-found
  error. Add router-level tests that exercise every mounted route and reject
  successful-looking fabricated results.

- [ ] **Make HTTP metrics repeatable and cover application routes.**
  Constructing `http::routes()` twice in one process panics because the
  Prometheus router installs a global recorder each time (reproduced in tests).
  Separate recorder initialization from routing and test multiple routers.
  In `lib/asimov-server/src/http/prometheus.rs`, the metrics layer wraps only
  `/metrics`, `/fast`, and `/slow`; other merged API routes are uninstrumented.
  Apply the layer to the assembled application and test actual API traffic.

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

- [ ] **Correct KB GraphQL scalar registration and validation.**
  Replace GraphQL names such as `PERSON ID`/`ID<16>` in `lib/asimov-kb/src/`
  with valid, consistently registered scalars whose validation checks the
  actual ID format.

- [ ] **Quote account handles as SQL string literals.**
  `lib/asimov-id/src/handle.rs` implements `eloquent::ToSql` by returning the
  bare handle. A probe returns `alice` where Eloquent's `String` conversion
  returns `'alice'`; queries therefore treat the value as an identifier or
  expression. Delegate to the string conversion and test value expressions
  for ordinary, numeric, and hyphenated handles against a local database.

- [ ] **Bound telemetry flush memory and checkpoint successful batches.**
  `lib/asimov-telemetry/src/client.rs::flush` holds every inactive log open and
  collects all events before sending 500-event batches. A later batch failure
  retains all files, replaying already accepted events on retry. Process a
  bounded number of logs/events and retain per-batch progress or stable event
  identity. Test a large backlog, partial HTTP failure, malformed/truncated
  logs, and file-descriptor limits with the existing local receiver fixture.
  `map_while(Result::ok)` also discards read failures before deleting the log;
  preserve unread events and distinguish corrupt records from I/O failures.

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
  TODOs; `programs/prompter.rs::test_execute` invokes `cat` even on Windows.
  Use self-hosted process fixtures for uncovered invocation contracts and retain
  network tests as explicit smoke tests. Add randomized framing/URL/ID
  round-trip tests and targeted failure injection for the P1 findings rather
  than duplicating existing transport tests.

- [ ] **Make discovery configuration reflect the running server.**
  `lib/asimov-server/src/mdns.rs` fixes the instance name, UDP service type, and
  port 1920, and unwraps local-IP discovery. Accept the actual transport,
  listener address/port, and instance identity; return discovery errors and
  expose a managed daemon lifetime. Test advertisement data without depending
  on multicast availability.

- [ ] **Implement the first ledger and credit database integration contracts.**
  Follow the scope in each crate's `src/lib.rs`: ledger needs explicit provider
  and contract selection, token-unit conversions, and local failed-call tests.
  Credit database adapters need exact encodings, checked decoding errors, and
  boundary round trips after the credit range/precision contract is fixed.

- [ ] **Repair README generation dependencies and failure handling.**
  `Makefile` references the missing
  `../.config/readmer/rust/README.md.liquid` (`make -Bn README.md` fails).
  Both `lib/asimov-remote/` and `lib/asimov-telemetry/` lack the
  `examples.md.j2` required by the shared template and generated READMEs;
  remote also lacks `package.metadata.readme.title`. Restore the missing inputs.
  Per-crate `Rakefile` tasks depend only on the shared Jinja template, omitting
  manifests, local examples, and the package table; they truncate output and
  ignore renderer exit status. Track all inputs, check subprocess status, and
  replace generated files atomically so a failed renderer preserves the prior
  document.

- [ ] **Refresh generated package metadata and valid feature examples.**
  `Cargo.toml`, `Rakefile`, and the shared template
  `../.config/readmer/rust/README.md.j2` link to the old `asimov.rs` repository
  layout. The shared template advertises
  `features = ["tracing"]` even for remote and telemetry, which do not export
  that feature, and describes workspace capabilities on empty scaffold crates.
  Generate accurate per-crate examples from metadata, use the actual Rust
  1.97.1 minimum, and refresh package tables/READMEs through their generators.
  `lib/asimov-keyring/README.md` still identifies itself as `asimov-social`;
  also refresh stale embedded package tables in the ID/KB READMEs.

- [ ] **Keep rustdoc warning-free and document public failure contracts.**
  Add `RUSTDOCFLAGS="-D warnings"` default/all-features documentation builds to
  CI. Prioritize missing error, persistence, and feature-availability docs on
  the public registry, installer, protocol, credit, and snapshot APIs.

- [ ] **Establish a clean Clippy baseline in small crate-local patches.**
  The strict workspace check fails in credit, env, flow, huggingface, id, kb,
  patterns, and prompt before reaching every dependent. In particular,
  `lib/asimov-id/src/key.rs` uses `since = "25.3"`, which triggers the
  deny-by-default `deprecated_semver` lint. Other reported lints include
  `clone_on_copy`, `collapsible_if`, `doc_lazy_continuation`, `from_over_into`,
  `io_other_error`, `needless_borrow`, `new_without_default`, `question_mark`,
  and `redundant_closure`. Review API-changing suggestions individually and
  continue through downstream crates once their dependencies pass.

## Validation evidence

Local checks ran on 2026-10-07 on macOS/aarch64 with Rust 1.98.1, plus the
Rust 1.97.1 MSRV check below. Cargo builds used `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0` where applicable, and `CARGO_INCREMENTAL=0` to
limit disk use. These checks do not establish runtime correctness of open items.

- `cargo fmt --all -- --check` passed.
- `cargo +1.97.1 check --workspace --all-targets --locked` and
  `cargo check --workspace --all-features --all-targets --locked` passed.
- All 41 members passed isolated `cargo check -p <crate> --lib --locked` with
  defaults, `--no-default-features`, `--no-default-features --features std`, and
  `--all-features`. Each also passed an isolated default `--all-targets` check.
- Additional isolated no-default library checks passed for module-kit
  `module`/`lint`; SDK `std,directory`/`std,flow`/`std,module`; server
  `http`/`persistence`/`tracing`; module `json`/`yaml`/`serde`/`std,serde`;
  patterns `serde`/`clap`; remote `http`; and social `client`.
  KB `--no-default-features --features new` failed as described above.
- `cargo test --workspace --locked --no-fail-fast` passed, including doctests,
  with no explicit test skips. The two network smoke tests (Hugging Face and
  module-template generation) remained ignored. Optional-integration tests were
  compile-checked with all features; the full all-features test suite was not
  run.
- `cargo check -p asimov-cloud --lib --no-default-features
  --target wasm32-unknown-unknown --locked` passed. This is a WebAssembly check,
  not a target without std.
- `cargo check -p asimov-core --lib --no-default-features
  --target thumbv7em-none-eabihf --locked` failed: transitive std features
  require an unavailable standard library. `cargo tree -p asimov-core
  --no-default-features --target thumbv7em-none-eabihf --locked -e features
  -i serde_json` traces `know/default` through `know/std` and pretty-printing.
- Both `cargo doc --workspace --no-deps --locked` and its `--all-features`
  variant passed with `RUSTDOCFLAGS="-D warnings"`.
- `cargo clippy --workspace --all-features --all-targets --locked
  -- -D warnings` failed on the existing lints summarized above.
- Temporary local probes reproduced credit overflow/string-rounding, snapshot
  timestamp loss/collisions/dangling-current selection, legacy-manifest loss,
  repeated-router panics, path normalization, YAML sequence corruption, and
  unquoted Eloquent handles. Turn these cases into regression tests with fixes.
- [CI run 37534709019][ci-review] at `a121139` passed on macOS, Linux, and
  native x86_64 Windows with Rust 1.97.1. Windows logs include passing proxy
  `serves_requests_with_upstream_credentials_and_streamed_logged_responses`
  and runner `process_tree` fixtures. This evidence is from CI, not a local
  Windows execution.

[ci-review]: https://github.com/asimov-platform/asimov-sdk/actions/runs/37534709019
