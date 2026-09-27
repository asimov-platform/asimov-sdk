# Rust code
- Don't ever directly read the contents of `Cargo.lock`, it can very large.
- Our current MSRV is Rust 1.97; update and enforce everywhere as needed.
- Our crates collect their default features under an `all` feature, and
  their `[features]` should start with the line `default = ["all", "std"]`.
- Our crates are meant to always be buildable with `#![no_std]` and hence
  always export an explicit `std` feature flag. Some of our lower-level crates
  may also have an explicit `alloc` feature, but for many crates it's not
  possible to do anything useful without heap allocations and they hence
  implicitly assume and omit such a feature.
- All references to `std`, `alloc`, and `core` types should always use
  qualified names or explicit, least-power imports. For example, prefer
  `core::error::Error` and `alloc::string::String` over `std` analogs.
- After making changes to a crate, as a last step run `cargo doc` on it.

# Workspace and features
- Paths below are relative to `rust/`. Run Cargo here so `.cargo/config.toml`
  is loaded, including its WebAssembly settings.
- `Cargo.toml` defines an edition-2024 workspace with members `lib/*` and
  `rust-version = "1.97.1"`; keep `../.github/workflows/ci.yaml` in sync.
- Inherit package metadata and shared dependencies with `workspace = true`.
  Register new crates in `[workspace.dependencies]` and `[patch.crates-io]`.
  SDK exposure requires both feature wiring and a re-export in `asimov-sdk`.
- Preserve `#![forbid(unsafe_code)]`; gate std-only code and re-exports
  together. Disable dependency defaults; forward required features explicitly,
  using `dependency?/std` for optional dependencies without activating them.
- `all` bundles defaults; `--all-features` also enables optional integrations.

# Code map
- `lib/asimov-sdk` is the feature-gated facade; `asimov-core` has shared traits,
  types, errors, and optional tracing support. Other crate paths use `lib/` too.
- Modules: `asimov-module` (manifests/resolution), `asimov-registry` (installed
  state), `asimov-installer` (downloads), `asimov-module-kit` (authoring).
- Execution: `asimov-patterns` (traits/options), `asimov-flow` (shared JSONL),
  `asimov-runner` (local), `asimov-remote` (remote), `asimov-social` (domain).
  Executors depend on patterns/flow. Preserve the boundaries and stream
  semantics in `lib/asimov-flow/DESIGN.md`.
- State/configuration: `asimov-env`, `asimov-config`, `asimov-directory`,
  `asimov-snapshot`. Identifiers: `asimov-id`, `asimov-kb`. Networking:
  `asimov-protocol` (Iroh), `asimov-server` (HTTP/OpenAI/MCP).
- Several crates are scaffolds; confirm APIs and feature gates in `src/lib.rs`.
  Follow specification links in module/pattern rustdoc for wire/CLI contracts.

# Validation
For each changed crate, run from `rust/` (replace `<crate>` with its package):

```sh
cargo fmt -p <crate> -- --check
cargo test -p <crate>
cargo check -p <crate> --lib --no-default-features
cargo doc -p <crate> --no-deps
```

- Test affected dependents and feature combinations separately; workspace
  feature unification can hide missing flags. Report existing build blockers.
- Tests live inline and in crate-local `tests/`. Use temporary directories and
  local fixtures; Hugging Face and module-template network tests are ignored.

# Generated crate documentation
- Crate READMEs use `../.config/readmer/rust/README.md.j2`, crate-local
  `examples.md.j2`, and Cargo metadata. Render a changed crate's README with
  `rake -B lib/<crate>/README.md` here; this needs `tomlrb` and `minijinja-cli`.
- `Rakefile` also generates `.cargo/packages.json` and `.cargo/packages.md` from
  manifests; refresh them with `rake .cargo/packages.json .cargo/packages.md`
  when package metadata changes.
