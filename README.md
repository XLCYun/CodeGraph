# CodeGraph

CodeGraph analyzes a fixed source directory and produces a common Code Graph through
seven fixed language entries and one UTF-8 NDJSON protocol. Rust, TypeScript/JavaScript, Python, Java, Go, C#,
and C/C++ use their native analysis libraries. No Narrant checkout, installation,
Tauri dependency, Git repository, or Review state is required.

This is the **0.1.0 development extraction**. No executable or language-pack release
is published by this stage. macOS ARM64 and Windows x64 are the intended targets;
complete dual-platform CI acceptance remains required before release. Independent
builds, package verification and the release contract are in
[distribution.md](docs/distribution.md).

## Build and verify

Use Rust and Node.js (for the serialized Cargo/cache launcher and process fixtures).
Rust 1.98.0 is required by the Rust/Python engines and common crate. Each small
public entry builds with Rust 1.97.1 or later. `sccache` is optional: the wrapper warns
and falls back if unavailable.

```sh
scripts/cargo-agent.sh build --locked --bin codegraph-rust
scripts/cargo-agent.sh test --locked --lib -- --test-threads=1
scripts/cargo-agent.sh test --locked --test cli -- --test-threads=1
scripts/cargo-agent.sh +1.98.0 check --locked --manifest-path analyzers/common/Cargo.toml
node --test scripts/tests/rust-cache.test.mjs
```

All Cargo commands, including engine builds and formatting, go through
`scripts/cargo-agent.sh`. Linked worktrees share the repository lock and sccache;
each checkout keeps its own `target`. Keep Cargo incremental enabled. Do not share
a target directory across divergent branches. The shared `codegraph-runtime` library is private (`publish = false`); each public
binary binds one concrete adapter. There is no generic executable or supported Rust API.
Heavy language engines have separate manifests,
pinned toolchains/dependencies, and licenses under `analyzers/`.

Tests marked `ignore` require real prepared language packs or are subprocess fixtures.
They must be explicitly run with their documented environment in release acceptance;
a green small-engine test run does not certify all seven language semantics.

## Run

```sh
target/debug/codegraph-rust --help
target/debug/codegraph-rust --version
python3 scripts/examples/empty-selection.py target/debug/codegraph-rust
```

The last command creates isolated temporary directories and exercises a valid empty
selection with stdin closed after the request. A non-empty selection requires a
trusted, prepared language pack:

```sh
/absolute/trusted/pack/bin/codegraph-typescript-javascript analyze < request.json
```

Each entry derives its package root from its own `bin/` directory and verifies
its compiled language identity against `control/codegraph.json`. Consumers do not
pass a language selector or package directory. The request file contains one JSON object followed by a newline. For interactive
cancellation, keep stdin open and send `cancel` with the same ID. Consumers that own
the child process must also retain process-tree ownership for forced cleanup.

See [the public protocol](docs/protocol.md), [language-pack layout](docs/pack-layout.md),
and [migration provenance and test ownership](docs/migration.md).

## License

CodeGraph uses Apache-2.0. Extracted Narrant code retains its original licensing;
third-party components keep their own licenses, fixed revisions, and notices.
The migration originates in [Narrant issue #205](https://github.com/XLCYun/narrant/issues/205).
Code Graph Difference remains in the consuming application.
