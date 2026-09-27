# Project files
- Don't ask to examine parent directories, stick to the project directory.
- When updating `AGENTS.md`, keep in mind that the file is meant to especially
  benefit lesser models than yourself, such as GPT-5.6 Sol, Terra, and Luna;
  but they also have smaller context windows, so be terse and token-efficient.
- Don't update `README.md` casually: beneficial additions require significant
  judgment and discernment, possibly beyond your capabilities. Longer and more
  detailed does *not* in fact equal better, because humans are not LLMs!
- Any documentation you might wish to add to the README likely better belongs
  as inline comments to the modules and/or types in question, where you are
  allowed elaborated length. For example, in Rust code `rustdoc` coverage of
  every public symbol is a worthy goal, but brevity and quality matter.
- If you must update e.g. `rust/README.md`, update also the Liquid template it
  is generated from, in the workspace root's `.config/readmer/rust/README.md`.
  Re-generate using `make -B README.md` in that directory, which invokes
  Readmer (https://github.com/artob/readmer).
- Keep to an 80-column wordwrap in comments and Markdown files, where feasible.

# Repository map
- `rust/` contains the main SDK implementation; follow `rust/AGENTS.md`.
- `dart/`, `js/`, `python/`, and `ruby/` are early language SDKs. Ruby includes
  a native Rust extension in `ruby/ext/asimov/`, with a separate workspace in
  `ruby/Cargo.toml`; apply the Rust code rules in `rust/AGENTS.md` there too.
- The `asimov` CLI lives in the separate `asimov-platform/asimov-cli` repo.
- Versions differ by language; use the owning package's manifest and `VERSION`.
- Keep the public-domain source headers.

# Local commands
Run tools in the relevant language directory. Bare `make` renders READMEs.
- `js/`: use Bun; `bun install`, then `bun run build` (ESM and declarations).
- `python/`: `uv sync`, then `uv build`; sources are in `src/asimov/`.
- `ruby/`: `bundle install`, then `bundle exec rake build` (native extension).
- `dart/`: `dart pub get`, then `dart analyze` and `dart test`.
- Non-Rust test suites are currently empty or absent.

# README generation caveat
The baseline example above needs review: Makefiles use `README.md.liquid`
templates, and `.config/readmer/rust/README.md.liquid` is currently missing.
The Rust regeneration command runs from `rust/` and fails until that template
exists. Per-crate Rust READMEs use a separate Rake/Jinja pipeline documented in
`rust/AGENTS.md`.

# Project repositories
- ASIMOV CLI: <https://github.com/asimov-platform/asimov-cli>
- ASIMOV SDK: <https://github.com/asimov-platform/asimov-sdk>
- ASIMOV Specs: <https://github.com/asimov-specs/asimov-specs>
- ASIMOV Modules: <https://github.com/asimov-modules/asimov-modules>
- Async-Flow: <https://github.com/artob/async-flow>
- Bitcache: <https://github.com/artob/bitcache>
- RDF.rs: <https://github.com/rust-rdf/rdf.rs>
- Readmer: <https://github.com/artob/readmer>
- SPARQL.rs: <https://github.com/rust-rdf/sparql.rs>
