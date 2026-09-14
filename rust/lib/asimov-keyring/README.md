# ASIMOV.rs: Social Media

[![License](https://img.shields.io/badge/license-Public%20Domain-blue.svg)](https://unlicense.org)
[![Compatibility](https://img.shields.io/badge/rust-1.97%2B-blue)](https://blog.rust-lang.org/2026/07/09/Rust-1.97.0/)
[![Package](https://img.shields.io/crates/v/asimov-social)](https://crates.io/crates/asimov-social)
[![Documentation](https://img.shields.io/docsrs/asimov-social?label=docs.rs)](https://docs.rs/asimov-social)

This package is part of [ASIMOV.rs], a polyglot development platform
for [trustworthy neurosymbolic machine intelligence].

<br/>

<sub>

[[Features](#-features)] |
[[Prerequisites](#%EF%B8%8F-prerequisites)] |
[[Installation](#%EF%B8%8F-installation)] |
[[Examples](#-examples)] |
[[Reference](#-reference)] |
[[Development](#%E2%80%8D-development)]

</sub>

## ✨ Features

- Defines [flow-based] [program patterns] for refining data into knowledge.
- Implements a [module system] enabling an ecosystem of [modules].
- Enables dataflow systems through reusable components called blocks.
- Compatible with the inventory of dataflow blocks provided by [Flows.rs].
- Built on the dataflow primitives provided by the [Async-Flow] crate.
- Supports opting out of any feature using comprehensive feature flags.
- Adheres to the Rust API Guidelines in its [naming conventions].
- Cuts red tape: 100% free and unencumbered public domain software.

## 🛠️ Prerequisites

- [Rust](https://rust-lang.org) 1.97+ (2024 edition)

## ⬇️ Installation

### Installation via Cargo

```bash
cargo add asimov-social
```

### Installation in `Cargo.toml`

```toml
[dependencies]
asimov-social = { version = "25" }
```

Alternatively, enable only specific features:

```toml
[dependencies]
asimov-social = { version = "25", default-features = false, features = ["tracing"] }
```

## 👉 Examples

### Importing the Library

```rust
use asimov_social::*;
```

## 📚 Reference

[docs.rs/asimov-social](https://docs.rs/asimov-social)

### Packages

| Package | Crate | Docs |
| :------ | :---- | :--- |
| [asimov-account](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-account) | [![Package](https://img.shields.io/crates/v/asimov-account)](https://crates.io/crates/asimov-account) | [![Documentation](https://img.shields.io/docsrs/asimov-account?label=docs.rs)](https://docs.rs/asimov-account) |
| [asimov-agent](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-agent) | [![Package](https://img.shields.io/crates/v/asimov-agent)](https://crates.io/crates/asimov-agent) | [![Documentation](https://img.shields.io/docsrs/asimov-agent?label=docs.rs)](https://docs.rs/asimov-agent) |
| [asimov-cache](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-cache) | [![Package](https://img.shields.io/crates/v/asimov-cache)](https://crates.io/crates/asimov-cache) | [![Documentation](https://img.shields.io/docsrs/asimov-cache?label=docs.rs)](https://docs.rs/asimov-cache) |
| [asimov-cloud](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-cloud) | [![Package](https://img.shields.io/crates/v/asimov-cloud)](https://crates.io/crates/asimov-cloud) | [![Documentation](https://img.shields.io/docsrs/asimov-cloud?label=docs.rs)](https://docs.rs/asimov-cloud) |
| [asimov-config](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-config) | [![Package](https://img.shields.io/crates/v/asimov-config)](https://crates.io/crates/asimov-config) | [![Documentation](https://img.shields.io/docsrs/asimov-config?label=docs.rs)](https://docs.rs/asimov-config) |
| [asimov-construct](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-construct) | [![Package](https://img.shields.io/crates/v/asimov-construct)](https://crates.io/crates/asimov-construct) | [![Documentation](https://img.shields.io/docsrs/asimov-construct?label=docs.rs)](https://docs.rs/asimov-construct) |
| [asimov-core](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-core) | [![Package](https://img.shields.io/crates/v/asimov-core)](https://crates.io/crates/asimov-core) | [![Documentation](https://img.shields.io/docsrs/asimov-core?label=docs.rs)](https://docs.rs/asimov-core) |
| [asimov-credit](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-credit) | [![Package](https://img.shields.io/crates/v/asimov-credit)](https://crates.io/crates/asimov-credit) | [![Documentation](https://img.shields.io/docsrs/asimov-credit?label=docs.rs)](https://docs.rs/asimov-credit) |
| [asimov-dataset](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-dataset) | [![Package](https://img.shields.io/crates/v/asimov-dataset)](https://crates.io/crates/asimov-dataset) | [![Documentation](https://img.shields.io/docsrs/asimov-dataset?label=docs.rs)](https://docs.rs/asimov-dataset) |
| [asimov-directory](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-directory) | [![Package](https://img.shields.io/crates/v/asimov-directory)](https://crates.io/crates/asimov-directory) | [![Documentation](https://img.shields.io/docsrs/asimov-directory?label=docs.rs)](https://docs.rs/asimov-directory) |
| [asimov-env](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-env) | [![Package](https://img.shields.io/crates/v/asimov-env)](https://crates.io/crates/asimov-env) | [![Documentation](https://img.shields.io/docsrs/asimov-env?label=docs.rs)](https://docs.rs/asimov-env) |
| [asimov-flow](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-flow) | [![Package](https://img.shields.io/crates/v/asimov-flow)](https://crates.io/crates/asimov-flow) | [![Documentation](https://img.shields.io/docsrs/asimov-flow?label=docs.rs)](https://docs.rs/asimov-flow) |
| [asimov-graph](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-graph) | [![Package](https://img.shields.io/crates/v/asimov-graph)](https://crates.io/crates/asimov-graph) | [![Documentation](https://img.shields.io/docsrs/asimov-graph?label=docs.rs)](https://docs.rs/asimov-graph) |
| [asimov-huggingface](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-huggingface) | [![Package](https://img.shields.io/crates/v/asimov-huggingface)](https://crates.io/crates/asimov-huggingface) | [![Documentation](https://img.shields.io/docsrs/asimov-huggingface?label=docs.rs)](https://docs.rs/asimov-huggingface) |
| [asimov-id](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-id) | [![Package](https://img.shields.io/crates/v/asimov-id)](https://crates.io/crates/asimov-id) | [![Documentation](https://img.shields.io/docsrs/asimov-id?label=docs.rs)](https://docs.rs/asimov-id) |
| [asimov-installer](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-installer) | [![Package](https://img.shields.io/crates/v/asimov-installer)](https://crates.io/crates/asimov-installer) | [![Documentation](https://img.shields.io/docsrs/asimov-installer?label=docs.rs)](https://docs.rs/asimov-installer) |
| [asimov-kb](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-kb) | [![Package](https://img.shields.io/crates/v/asimov-kb)](https://crates.io/crates/asimov-kb) | [![Documentation](https://img.shields.io/docsrs/asimov-kb?label=docs.rs)](https://docs.rs/asimov-kb) |
| [asimov-ledger](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-ledger) | [![Package](https://img.shields.io/crates/v/asimov-ledger)](https://crates.io/crates/asimov-ledger) | [![Documentation](https://img.shields.io/docsrs/asimov-ledger?label=docs.rs)](https://docs.rs/asimov-ledger) |
| [asimov-module](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-module) | [![Package](https://img.shields.io/crates/v/asimov-module)](https://crates.io/crates/asimov-module) | [![Documentation](https://img.shields.io/docsrs/asimov-module?label=docs.rs)](https://docs.rs/asimov-module) |
| [asimov-module-kit](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-module-kit) | [![Package](https://img.shields.io/crates/v/asimov-module-kit)](https://crates.io/crates/asimov-module-kit) | [![Documentation](https://img.shields.io/docsrs/asimov-module-kit?label=docs.rs)](https://docs.rs/asimov-module-kit) |
| [asimov-nexus](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-nexus) | [![Package](https://img.shields.io/crates/v/asimov-nexus)](https://crates.io/crates/asimov-nexus) | [![Documentation](https://img.shields.io/docsrs/asimov-nexus?label=docs.rs)](https://docs.rs/asimov-nexus) |
| [asimov-ontology](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-ontology) | [![Package](https://img.shields.io/crates/v/asimov-ontology)](https://crates.io/crates/asimov-ontology) | [![Documentation](https://img.shields.io/docsrs/asimov-ontology?label=docs.rs)](https://docs.rs/asimov-ontology) |
| [asimov-patterns](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-patterns) | [![Package](https://img.shields.io/crates/v/asimov-patterns)](https://crates.io/crates/asimov-patterns) | [![Documentation](https://img.shields.io/docsrs/asimov-patterns?label=docs.rs)](https://docs.rs/asimov-patterns) |
| [asimov-platform](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-platform) | [![Package](https://img.shields.io/crates/v/asimov-platform)](https://crates.io/crates/asimov-platform) | [![Documentation](https://img.shields.io/docsrs/asimov-platform?label=docs.rs)](https://docs.rs/asimov-platform) |
| [asimov-prompt](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-prompt) | [![Package](https://img.shields.io/crates/v/asimov-prompt)](https://crates.io/crates/asimov-prompt) | [![Documentation](https://img.shields.io/docsrs/asimov-prompt?label=docs.rs)](https://docs.rs/asimov-prompt) |
| [asimov-protocol](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-protocol) | [![Package](https://img.shields.io/crates/v/asimov-protocol)](https://crates.io/crates/asimov-protocol) | [![Documentation](https://img.shields.io/docsrs/asimov-protocol?label=docs.rs)](https://docs.rs/asimov-protocol) |
| [asimov-proxy](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-proxy) | [![Package](https://img.shields.io/crates/v/asimov-proxy)](https://crates.io/crates/asimov-proxy) | [![Documentation](https://img.shields.io/docsrs/asimov-proxy?label=docs.rs)](https://docs.rs/asimov-proxy) |
| [asimov-registry](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-registry) | [![Package](https://img.shields.io/crates/v/asimov-registry)](https://crates.io/crates/asimov-registry) | [![Documentation](https://img.shields.io/docsrs/asimov-registry?label=docs.rs)](https://docs.rs/asimov-registry) |
| [asimov-repository](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-repository) | [![Package](https://img.shields.io/crates/v/asimov-repository)](https://crates.io/crates/asimov-repository) | [![Documentation](https://img.shields.io/docsrs/asimov-repository?label=docs.rs)](https://docs.rs/asimov-repository) |
| [asimov-runner](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-runner) | [![Package](https://img.shields.io/crates/v/asimov-runner)](https://crates.io/crates/asimov-runner) | [![Documentation](https://img.shields.io/docsrs/asimov-runner?label=docs.rs)](https://docs.rs/asimov-runner) |
| [asimov-runtime](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-runtime) | [![Package](https://img.shields.io/crates/v/asimov-runtime)](https://crates.io/crates/asimov-runtime) | [![Documentation](https://img.shields.io/docsrs/asimov-runtime?label=docs.rs)](https://docs.rs/asimov-runtime) |
| [asimov-sdk](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-sdk) | [![Package](https://img.shields.io/crates/v/asimov-sdk)](https://crates.io/crates/asimov-sdk) | [![Documentation](https://img.shields.io/docsrs/asimov-sdk?label=docs.rs)](https://docs.rs/asimov-sdk) |
| [asimov-server](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-server) | [![Package](https://img.shields.io/crates/v/asimov-server)](https://crates.io/crates/asimov-server) | [![Documentation](https://img.shields.io/docsrs/asimov-server?label=docs.rs)](https://docs.rs/asimov-server) |
| [asimov-snapshot](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-snapshot) | [![Package](https://img.shields.io/crates/v/asimov-snapshot)](https://crates.io/crates/asimov-snapshot) | [![Documentation](https://img.shields.io/docsrs/asimov-snapshot?label=docs.rs)](https://docs.rs/asimov-snapshot) |
| [asimov-social](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-social) | [![Package](https://img.shields.io/crates/v/asimov-social)](https://crates.io/crates/asimov-social) | [![Documentation](https://img.shields.io/docsrs/asimov-social?label=docs.rs)](https://docs.rs/asimov-social) |
| [asimov-token](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-token) | [![Package](https://img.shields.io/crates/v/asimov-token)](https://crates.io/crates/asimov-token) | [![Documentation](https://img.shields.io/docsrs/asimov-token?label=docs.rs)](https://docs.rs/asimov-token) |
| [asimov-universe](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-universe) | [![Package](https://img.shields.io/crates/v/asimov-universe)](https://crates.io/crates/asimov-universe) | [![Documentation](https://img.shields.io/docsrs/asimov-universe?label=docs.rs)](https://docs.rs/asimov-universe) |
| [asimov-vault](https://github.com/asimov-platform/asimov.rs/tree/master/lib/asimov-vault) | [![Package](https://img.shields.io/crates/v/asimov-vault)](https://crates.io/crates/asimov-vault) | [![Documentation](https://img.shields.io/docsrs/asimov-vault?label=docs.rs)](https://docs.rs/asimov-vault) |

### Glossary

- **System**: A collection of blocks that are connected together.
  Systems are the top-level entities in dataflow programs.

- **Block**: An encapsulated system component that processes messages.
  Blocks are the autonomous units of computation in a system.

- **Port**: A named connection point on a block that sends or receives
  messages. Ports are the only interfaces through which blocks communicate
  with each other.

- **Message**: A unit of data that flows between blocks in a system, from port
  to port. Any Rust type that implements the `Send + Sync + 'static` traits can
  be used as a message.

## 👨‍💻 Development

```bash
git clone https://github.com/asimov-platform/asimov.rs.git
```

---

[![Share on X](https://img.shields.io/badge/share%20on-x-03A9F4?logo=x)](https://x.com/intent/post?url=https://github.com/asimov-platform/asimov.rs&text=ASIMOV%20Software%20Development%20Kit%20%28SDK%29%20for%20Rust)
[![Share on Reddit](https://img.shields.io/badge/share%20on-reddit-red?logo=reddit)](https://reddit.com/submit?url=https://github.com/asimov-platform/asimov.rs&title=ASIMOV%20Software%20Development%20Kit%20%28SDK%29%20for%20Rust)
[![Share on Hacker News](https://img.shields.io/badge/share%20on-hn-orange?logo=ycombinator)](https://news.ycombinator.com/submitlink?u=https://github.com/asimov-platform/asimov.rs&t=ASIMOV%20Software%20Development%20Kit%20%28SDK%29%20for%20Rust)
[![Share on Facebook](https://img.shields.io/badge/share%20on-fb-1976D2?logo=facebook)](https://www.facebook.com/sharer/sharer.php?u=https://github.com/asimov-platform/asimov.rs)
[![Share on LinkedIn](https://img.shields.io/badge/share%20on-linkedin-3949AB?logo=linkedin)](https://www.linkedin.com/sharing/share-offsite/?url=https://github.com/asimov-platform/asimov.rs)

[ASIMOV]: https://asimov.sh
[ASIMOV.rs]: https://github.com/asimov-platform/asimov.rs
[Async-Flow]: https://github.com/artob/async-flow
[Flows.rs]: https://github.com/artob/flows.rs
[flow-based]: https://github.com/artob/awesome-fbp
[naming conventions]: https://rust-lang.github.io/api-guidelines/naming.html
[modules]: https://github.com/asimov-modules
[module system]: https://asimov-specs.github.io/
[program patterns]: https://asimov-specs.github.io/program-patterns/
[trustworthy neurosymbolic machine intelligence]: https://asimov.blog/introducing-asimov/
