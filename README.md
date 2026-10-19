# CodeGraph

CodeGraph analyzes a fixed source directory and produces a common Code Graph through
one CLI and UTF-8 NDJSON protocol. Rust, TypeScript/JavaScript, Python, Java, Go, C#,
and C/C++ use their native analysis libraries. No Narrant checkout, installation,
Tauri dependency, Git repository, or Review state is required.

This is the **0.1.0 development extraction**. No executable or language-pack release
is published by this stage. macOS ARM64 and Windows x64 are the intended targets;
complete dual-platform CI acceptance remains required before release. Independent
builds, package verification and the release contract are in
[distribution.md](docs/distribution.md).

## Build and verify

Use Rust and Node.js (for the serialized Cargo/cache launcher and process fixtures).
Rust 1.98.0 is required by the native Rust/Python helpers and common crate. The small
CLI itself builds with Rust 1.97.1 or later. `sccache` is optional: the wrapper warns
and falls back if unavailable.

```sh
scripts/cargo-agent.sh build --locked --bin codegraph
scripts/cargo-agent.sh test --locked --bin codegraph -- --test-threads=1
scripts/cargo-agent.sh test --locked --test cli -- --test-threads=1
scripts/cargo-agent.sh +1.98.0 check --locked --manifest-path native-helpers/common/Cargo.toml
node --test scripts/tests/rust-cache.test.mjs
```

All Cargo commands, including helper builds and formatting, go through
`scripts/cargo-agent.sh`. Linked worktrees share the repository lock and sccache;
each checkout keeps its own `target`. Keep Cargo incremental enabled. Do not share
a target directory across divergent branches. The project is binary-only: there is
no `--lib` target or public Rust API. Heavy helper builds have separate manifests,
pinned toolchains/dependencies, and licenses under `native-helpers/`.

Tests marked `ignore` require real prepared language packs or are subprocess fixtures.
They must be explicitly run with their documented environment in release acceptance;
a green small-engine test run does not certify all seven language semantics.

## Run

```sh
target/debug/codegraph --help
target/debug/codegraph --version
python3 scripts/examples/empty-selection.py target/debug/codegraph
```

The last command creates isolated temporary directories and exercises a valid empty
selection with stdin closed after the request. A non-empty selection requires a
trusted, prepared language pack:

```sh
codegraph analyze --pack typescript-javascript --pack-root /absolute/trusted/pack < request.json
```

The request file contains one JSON object followed by a newline. For interactive
cancellation, keep stdin open and send `cancel` with the same ID. Consumers that own
the child process must also retain process-tree ownership for forced cleanup.

See [the public protocol](docs/protocol.md), [language-pack layout](docs/pack-layout.md),
and [migration provenance and test ownership](docs/migration.md).

## License

CodeGraph uses Apache-2.0. Extracted Narrant code retains its original licensing;
third-party components keep their own licenses, fixed revisions, and notices.
The migration originates in [Narrant issue #205](https://github.com/XLCYun/narrant/issues/205).
Code Graph Difference remains in the consuming application.
