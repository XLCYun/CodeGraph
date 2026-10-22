# CodeGraph

CodeGraph provides seven language programs that analyze a fixed source directory
and return a common Code Graph. Each program directly implements the same UTF-8
[public JSON protocol](docs/protocol.md): one request followed by stdin EOF, one
final result, then exit. Rust, TypeScript/JavaScript, Python, Java, Go, C# and C/C++
use their own native analysis libraries and discover target SDKs themselves.

This is the unreleased **0.1.0 development series**, targeting macOS ARM64 and
Windows x64. No Narrant checkout, installation, Tauri dependency, Git repository
or Review state is needed. The first immutable prerelease requires human merge
and exact-source complete dual-platform CI; `published` remains false until actual
release consumption. See [distribution.md](docs/distribution.md).

## Build and verify

Use Node 24.18.0, pnpm 11.13.1 and the pinned language build tools in
[the toolchain manifest](resources/native-toolchains.json). All Cargo commands use
`scripts/cargo-agent.sh`, which serializes repository builds and uses sccache when
available. Keep incremental enabled and a separate target per divergent worktree.

```sh
pnpm install --frozen-lockfile
pnpm test
pnpm check:scripts
scripts/cargo-agent.sh fmt --all --check
scripts/cargo-agent.sh check --locked --workspace --lib --bins
scripts/cargo-agent.sh +1.98.0 check --manifest-path analyzers/rust/Cargo.toml --lib --locked
scripts/cargo-agent.sh +1.98.0 check --manifest-path analyzers/python/Cargo.toml --lib --locked
node scripts/language-packs/build-language-support-packs.mjs --native --output artifacts/direct-development
```

The root Cargo workspace contains only TS/Java thin launchers. Rust and Python
build with their own manifests, locks and Rust 1.98.0. Go and C/C++ have native
entries. C# publishes a self-contained apphost plus its fixed .NET runtime. There
is no cross-language Rust Host or public Rust SDK. TS/Java launchers only locate
bundled runtimes, inherit stdio and propagate exit status; macOS uses exec and
Windows uses spawn/wait. [Package layout](docs/pack-layout.md) lists runtime files.

The complete CI command is
`node scripts/language-packs/verify-native-analyzers.mjs artifacts --build --language all`.
It requires prepared fixed SDK fixtures, runs all native semantic suites, the
non-shipped public integration crate and real seven-package acceptance.
[Historical test mappings](docs/direct-test-migration.md) identify preserved
semantics and retired private control behavior. Focused local results do not
replace complete platform CI evidence.

## Run

Verify and install the full language package, then invoke its fixed entry:

```sh
python3 scripts/examples/analyze-directory.py /absolute/trusted/pack/bin/codegraph-typescript-javascript /absolute/project
python3 scripts/examples/empty-selection.py analyzers/rust/target/release/codegraph-rust
```

Omitting `files` uses the project's supported configuration; explicit nonempty
`files` is exact selection, and `files: []` returns an empty graph without project
or SDK discovery. A final newline on input is optional. Close stdin after the
request. Verify final `protocol`, `engine_version`, `pack_id`, request ID, outcome
and successful exit before consuming a graph. Cancellation, deadlines and
transport failures require caller-owned process-tree termination, pipe draining
and an exit barrier before releasing source/package leases.

Target SDK discovery is read-only. It never installs SDKs or runs project build
scripts. See [selection policies](docs/engine-selection.md),
[the public contract](docs/protocol.md) and [provenance](docs/migration.md).
Code Graph Difference and multi-language merging belong to consumers.

## License

CodeGraph uses Apache-2.0. Extracted Narrant code retains its original licensing;
third-party components retain fixed revisions, licenses and notices. The migration
originates in [Narrant #205](https://github.com/XLCYun/narrant/issues/205).
