# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 25.3.4 - 2026-08-10

### Changed
- Distinguish an uninstalled module in `Registry::read_readme` (@SamuelSarle)

## 25.3.3 - 2026-08-07

### Changed
- Add `secret` and `optional` to module configuration variables (@SamuelSarle)

## 25.3.2 - 2026-08-06

### Changed
- Install modules as directories (@SamuelSarle)
- Bump the MSRV to Rust 1.97 (@SamuelSarle)

## 25.3.1 - 2026-08-04

### Added
- `asimov_module::Index` (@SamuelSarle)

## 25.3.0 - 2026-07-28

### Added
- `asimov_module_kit` (@oleksandrvoyager)
- `asimov_protocol::CsvHandleResolver` (@artob)
- `asimov_protocol::HandleResolver` (@artob)
- `asimov_protocol::PeerProtocol` (@artob)
- `asimov_protocol::GossipProtocol` (@artob)

### Changed
- Redefine `asimov_id::Id` (@artob)
- Rename `asimov_id::Key` to `PublicKey` (@artob)
- Rename `asimov_id::Id` to `Handle` (@artob)
- Bump many dependencies to their latest releases (@artob)

## 25.2.0 - 2026-07-03

### Added
- `asimov_config::ConfigProfile` (@artob)
- `asimov_core::ModuleName` (@artob)
- `asimov_directory::ConfigDirectory` (@artob)
- `asimov_directory::ModuleDirectory` (@artob)
- `asimov_directory::ProgramDirectory` (@artob)
- `asimov_directory::StateDirectory` (@artob)
- `asimov_directory::fs::ConfigDirectory` (@artob)
- `asimov_directory::fs::ModuleDirectory` (@artob)
- `asimov_directory::fs::ModuleManifestIterator` (@SamuelSarle)
- `asimov_directory::fs::ProgramDirectory` (@artob)
- `asimov_directory::fs::StateDirectory` (@artob)
- `asimov_kb::OrganizationId` (@artob)
- `asimov_protocol::PingProtocol` (@artob)

### Changed
- Bump the RDF.rb dependency to 0.4.4 (@artob)
- Bump the Iroh dependency to 1.0.1 (@artob)
- Bump all dependencies to their latest patch releases (@artob)
- Improve interoperability with async_graphql (@artob)

## 25.1.0 - 2026-01-30

### Added
- Export all packages from the top-level crate (@artob)
- Define types for public keys and blob IDs (@artob)

### Fixed
- Fix builds with `--no-default-features` (@artob)
- Fix builds on WebAssembly targets (@artob)

### Removed
- Houseclean obsolete code overcome by events (@artob)

## 25.0.3 - 2026-01-04

### Added
- New crates: asimov-cloud, asimov-credit, asimov-id, asimov-kb, asimov-nexus (by @artob)

### Fixed
- Update module manifest metadata to match the spec (by @artob)

## 25.0.2 - 2025-11-12

### Changed

- Switch `asimov-installer` away from using `api.github.com` endpoints (#43 by @SamuelSarle)

## 25.0.1 - 2025-11-12

### Fixed

- Fix `asimov-installer` having wrong path on Windows (@imunproductive)

## 25.0.0 - 2025-11-05

### Added

- General availability

## 25.0.0-dev.25 - 2025-11-05

### Added

- Add support for model dependencies. (#40 by @SamuelSarle)

## 25.0.0-dev.24 - 2025-10-22

### Added

- Add support for MIME types in resolver (#39 by @SamuelSarle)
- Define a `Pipeline` struct (@artob)
- Add ability to specify module registry path (#36 by @imunproductive)
- Add `asimov-huggingface` with functionality to download models and files (#38 by @oleksandrvoyager)
- Add a `normalize_module_name` helper (#35 by @SamuelSarle)

### Fixed

- Fix module manifest parsing failing if an array is empty. (#37 by @imunproductive)

## 25.0.0-dev.23 - 2025-09-23

### Changed

- Make module resolver return longer matches first (#34 by @SamuelSarle)
- Make snapshot service run cataloger when fetcher fails (#33 by @SamuelSarle)

## 25.0.0-dev.22 - 2025-09-04

### Added

- Implement a `normalize_url` helper (#32 by @SamuelSarle)

### Changed

- Update the snapshot filesystem layout (#30 by @SamuelSarle)

### Fixed

- Fix installer timeout (#31 by @SamuelSarle)
- Fix issue with leading dot in module resolver

## 25.0.0-dev.21 - 2025-08-06

### Added

- Implement time-to-live (TTL) options for snapshots (#28 by @SamuelSarle)

### Changed

- Split installer to installer and registry (#29 by @SamuelSarle)

### Fixed

- Set 0444 file permissions for snapshot files (#27 by @SamuelSarle)
- Ensure JSON manifests are newline-terminated (by @artob)

## 25.0.0-dev.20 - 2025-08-02

### Changed

- Make enabled module symlinks relative (#26 by @SamuelSarle)
- Support module manifests in both YAML and JSON format (#25 by @SamuelSarle)

### Fixed

- Use serde_yaml_ng instead of serde_yml (#24 by @imunproductive)

## 25.0.0-dev.19 - 2025-07-29

### Changed

- asimov-runner: Use `$HOME/.asimov/libexec/` binaries (#23 by @SamuelSarle)

### Added

- Add a `SkipNulls` adapter for serde_json (by @artob)

## 25.0.0-dev.18 - 2025-07-26

### Added

- asimov-installer: Implement a module installer (#22 by @SamuelSarle)

### Changed

- Disable default features in dependencies by default (by @artob)

## 25.0.0-dev.17 - 2025-07-22

### Added

- Add read methods to the snapshotter. (#21 by @SamuelSarle)

### Fixed

- Fix asimov-snapshot on Windows (by @imunproductive)

## 25.0.0-dev.16 - 2025-07-21

### Added

- Implement a snapshot service (#20 by @SamuelSarle)

### Changed

- Improve builder usability (by @artob)
- Pass program options through the executor (by @artob)

## 25.0.0-dev.15 - 2025-07-18

### Changed

- Enhance the module resolver (#18 by @SamuelSarle)
- Convert to `SysexitsError` from asimov-module errors (#19 by @SamuelSarle)
- Overhaul of asimov-patterns (by @artob)
- Overhaul of asimov-runner (by @artob)

## 25.0.0-dev.14 - 2025-07-14

### Added

- No-op tracing macros when tracing is disabled (by @artob)
- Support module configuration in manifests (#16 by @SamuelSarle)
- Support for environment variables in module configuration (#17 by @SamuelSarle)

### Fixed

- `asimov_root()` on Windows (#15 by @imunproductive)
- Build error when building without default features (by @artob)

## 25.0.0-dev.13 - 2025-07-01

### Added

- `asimov_module::init_tracing_subscriber` (#13)

## 25.0.0-dev.12 - 2025-06-27

### Changed

- Bump the Clientele.rs dependency

### Added

- Resolve file extensions as well (#11)

### Fixed

- Prefixes mangling patterns (#12)

## 25.0.0-dev.11 - 2025-06-19

### Changed

- Update project links

### Added

- `asimov_module::ModuleManifest` (#10)

## 25.0.0-dev.10 - 2025-06-18

### Changed

- Implement a module resolver (#8)

### Added

- Implement an MCP server (#6)

### Fixed

- Fix `PythonEnv::initialize` (#9)

## 25.0.0-dev.9 - 2025-05-30

## 25.0.0-dev.8 - 2025-05-23

## 25.0.0-dev.7 - 2025-05-19

## 25.0.0-dev.6 - 2025-05-15

## 25.0.0-dev.5 - 2025-04-25

## 25.0.0-dev.4 - 2025-04-25

## 25.0.0-dev.3 - 2025-04-21

## 25.0.0-dev.2 - 2025-04-21

## 25.0.0-dev.1 - 2025-04-14

## 25.0.0-dev.0 - 2025-01-01

## 24.0.0-dev.22 - 2024-10-17

## 24.0.0-dev.21 - 2024-10-09

### Changed

- Flow: Upgrade to Protoflow 0.4.1

## 24.0.0-dev.20 - 2024-10-01

### Changed

- Flow: Upgrade to Protoflow 0.4.0

## 24.0.0-dev.19 - 2024-09-25

### Changed

- Flow: Upgrade to Protoflow 0.3.1

## 24.0.0-dev.18 - 2024-09-20

## 24.0.0-dev.17 - 2024-09-19

## 24.0.0-dev.16 - 2024-09-17

## 24.0.0-dev.15 - 2024-09-12

## 24.0.0-dev.14 - 2024-09-09

### Changed

- Flow: Upgrade to Protoflow 0.3.0

## 24.0.0-dev.13 - 2024-09-07

## 24.0.0-dev.12 - 2024-09-06

## 24.0.0-dev.11 - 2024-09-06

## 24.0.0-dev.10 - 2024-09-05

## 24.0.0-dev.9 - 2024-09-05

### Changed

- Flow: Upgrade to Protoflow 0.2.1

## 24.0.0-dev.8 - 2024-08-21

### Changed

- Flow: Upgrade to Protoflow 0.2.0

## 24.0.0-dev.7 - 2024-08-09

### Added

- Flow: `MpscTransport` transport implementation
- Flow: `#[derive(Block)]` now implements `sysml_model` traits

## 24.0.0-dev.6 - 2024-08-02

### Changed

- Flow: Blocks now return a `BlockResult` (a typedef for `Result<(), BlockError>`)
- Flow: `InputPort#receive()` now renamed to `InputPort#recv()`

### Added

- Flow: `BlockError::Panic` error
- Flow: `Buffer` block type
- Flow: `Message` trait, combining `prost::Message`, `Clone`, and `Default`
- Flow: `System::build()` DSL
- Flow: `Transport` interface, and `MockTransport` implementation

## 24.0.0-dev.5 - 2024-07-29

## 24.0.0-dev.4 - 2024-07-26

### Changed

- Flow: `Block#execute` parameters have changed

## 24.0.0-dev.3 - 2024-07-25

### Fixed

- Flow: fix build issues due to the `alloc` crate

### Added

- Flow: mock transport for testing
- Flow: `Const` and `Random` blocks

## 24.0.0-dev.2 - 2024-07-24

### Added

- Flow: `derive` feature
- Flow: `#[derive(Block)]` macro
- Flow: `#[derive(FunctionBlock)]` macro
- Flow: `BlockDescriptor` trait
- Flow: `BlockError` enum
- Flow: `PortError` enum

### Changed

- Flow: scheduler methods now return a `Result`

## 24.0.0-dev.1 - 2024-07-23

### Changed

- Flow: `Block#execute` now returns a `Result`
- Flow: `Block#execute` now takes a parameter

## 24.0.0-dev.0 - 2024-06-29
