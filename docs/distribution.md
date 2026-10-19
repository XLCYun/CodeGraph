# CodeGraph 0.1.0 distribution contract

Status: **not published**. This repository prepares artifacts and CI; it does not
claim that `v0.1.0` or any download URL below exists. The first release is an
immutable prerelease after review, merge and successful complete CI. Existing
Narrant language-pack releases are a separate channel and are never overwritten.

## Public identities and assets

Repository: `XLCYun/CodeGraph`. Engine, package and Node build-project versions are
all `0.1.0`; tag is `v0.1.0`; protocol is `codegraph.analyzer/1`.
Supported targets are `aarch64-apple-darwin` and `x86_64-pc-windows-msvc`.
Pack IDs are `rust`, `typescript-javascript`, `python`, `java`, `go`, `csharp`, `c-cpp`.
Private `narrant-*` helper names and their pins remain unchanged.

For each target the release contains one standalone CLI archive and seven pairs:

```text
codegraph-<target>.tar.gz
<id>-<target>-pack.tar.gz
<id>-<target>-manifest.json
SHA256SUMS
```

One release therefore has 31 assets: 2 CLI archives, 14 language archives, 14
language manifests and one checksum file covering the other 30 assets. Exact URL
rules for downstream configuration (templates, currently unavailable):

```text
https://github.com/XLCYun/CodeGraph/releases/download/v0.1.0/<id>-<target>-pack.tar.gz
https://github.com/XLCYun/CodeGraph/releases/download/v0.1.0/<id>-<target>-manifest.json
https://github.com/XLCYun/CodeGraph/releases/download/v0.1.0/codegraph-<target>.tar.gz
https://github.com/XLCYun/CodeGraph/releases/download/v0.1.0/SHA256SUMS
```

Every language archive includes the exact same `bin/codegraph` (Windows
`bin/codegraph.exe`) as that target's standalone archive, the public
`control/codegraph.json`, private helper/runtime files and dependency license texts.
Standalone CLI archives contain the CLI, `control/engine.json` and its licenses;
they require a separately verified language pack root for non-empty analysis.
The standalone control file records `version`, `engineVersion`, `protocol`,
`sourceCommit`, `sourceDirty`. These are distribution metadata; the analysis request
and ready/completed schemas are unchanged.

Language manifests retain `schemaVersion: 1`, `id`, `version`, `target`,
`archiveSize`, `archiveSha256` and `files` (`path`, `size`, `sha256`, `executable`).
They add mandatory `engineVersion`, `protocol`, `sourceCommit` (full Git SHA),
`sourceDirty` (boolean). Published manifests always have `sourceDirty: false` and
`version == engineVersion == 0.1.0`. Canonical compact JSON plus a final newline
rejects duplicate keys in the build verifier. The public pack descriptor remains
exactly the five snake_case fields documented in [pack-layout.md](pack-layout.md).
Consumers must update their manifest reader for these explicit additions.

The unchanged consumer limits are 256 MiB compressed archive, 1 GiB summed file
bytes, 2 MiB manifest. Both packaging and verification fail on overflow. Verification
also rejects unknown manifest fields, unsafe/case-colliding paths, device names,
links, duplicate/extra/missing files, byte/hash mismatches and executable-mode
mismatches. Archive entries use fixed timestamps and 0755/0644 modes. An integrity
manifest is not a signature: consumers must trust the fixed release/source channel,
then verify the checksum and manifest before executing a pack.

## Consumer installation and local candidates

After publication, download a chosen manifest, archive and `SHA256SUMS` from the
same fixed tag; verify the SHA-256 of both downloaded files, then manifest identity,
archive size/hash, bounded unpacking, every file hash and executable permissions.
Extract into a new directory outside the analyzed repository, and only promote that
complete verified directory to a trusted installation. Never merge with an old
Narrant `1.1.0` pack or another engine version. Keep the installation alive for the
whole CLI process and its descendants; an active pack cannot be removed mid-analysis.

Invoke `<verified-root>/bin/codegraph[.exe] analyze --pack <id> --pack-root
<absolute-verified-root>` and use [protocol.md](protocol.md). A separate CLI archive
uses the same arguments with the language root. Verify ready `engine_version`,
`pack_id` and `protocol`, then require a valid completed graph and successful exit.
Do not inspect or invoke private helper argv/environment/SDK configuration.

Local builds are under `<output>/<id>/0.1.0/<target>/{manifest.json,pack.tar.gz}`;
CLI build artifacts use ID `codegraph` in the same layout. Build-time helper
acceptance extracts language packs to
`<output>/smoke-installed/<id>/0.1.0/<target>`. This is a public pack root because it
contains the descriptor and packaged CLI. A consumer fixture can record only:

```json
{"id":"typescript-javascript","version":"0.1.0","engineVersion":"0.1.0","protocol":"codegraph.analyzer/1","target":"aarch64-apple-darwin","root":"/absolute/output/smoke-installed/typescript-javascript/0.1.0/aarch64-apple-darwin"}
```

The absolute `root` is local fixture configuration, not a release-manifest field.
`native-helpers-config.json` is CodeGraph's private semantic-test fixture and is
not uploaded or consumed by Narrant. Builds before a commit carry
`sourceDirty: true`; they are useful development fixtures and cannot satisfy the
release evidence gate. Rebuild after the final clean commit for production evidence.

## Build and acceptance

Use Node 24.18.0 and pnpm 11.13.1 (`pnpm install --frozen-lockfile`). The only Node
build dependency is pinned `tar@7.5.22`. All Cargo commands use
`scripts/cargo-agent.sh`; root is binary-only, so use `--bin codegraph`, not `--lib`.
Helper manifests retain separate targets, `+1.98.0`, upstream revisions and locks.
Keep incremental and each worktree's own targets. `sccache` is optional and shared
by the wrapper when installed.

For the light checks and a real TypeScript pack on a supported native host:

```sh
pnpm test
pnpm check:scripts
bash scripts/cargo-agent.sh fmt --all --check
bash scripts/cargo-agent.sh check --locked --bin codegraph
node scripts/language-packs/build-language-support-packs.mjs --native --id typescript-javascript --output artifacts
node scripts/language-packs/verify-native-analyzers.mjs artifacts --language typescript-javascript
```

The build command always includes the standalone CLI and defaults to all seven
packs if `--id` is absent. Rust/Python need installed helper compiler 1.98.0.
Java, Go, C#, C/C++ need their fixed build SDKs; install official checksum-pinned
inputs with `provision-native-toolchains.mjs <empty-directory> --language <id|all>`.
It writes `environment.json` and, in Actions, `GITHUB_ENV`; local callers must load
those environment entries into subsequent commands. Java dependencies are fetched
by the full verifier before building. `resources/native-toolchains.json` is the
exact upstream pin inventory. Do not replace pins with the current system version.

Target analysis prerequisites are separate from helper build tools: Rust uses the
project's installed Cargo/rustc and rust-src for std scopes; Java discovers local
JDK declarations; Go discovers installed SDK sources; C# discovers .NET reference
packs; C/C++ uses approved macOS SDK or Windows SDK/MSVC headers. TS/JS and Python
carry their required runtime/declarations. SDK discovery does not install tools.

Full Rust acceptance additionally requires `NARRANT_PROJECT_RUST_TOOLCHAIN=stable`,
an isolated absolute `CARGO_HOME`, stable `rust-src` and historical 1.81.0, 1.94.0,
1.96.0 toolchains; run `prepare-rust-native-tests.mjs --prepare` to prepare only
locked test metadata and verify its offline availability. These fixture versions
are not a product target-version allowlist.

`.github/workflows/ci.yml` runs on hosted `macos-15` ARM64 and `windows-2022` x64.
Host architecture is independently checked by native builders. Windows runs the
shared process probe before expensive downloads, then selects exact MSVC/SDK/redist
from the unchanged pins. macOS selects Xcode 16.4 / SDK 15.5. All seven helper suites,
69 retained semantic tests (70 on Windows), the full engine and public CLI lifecycle
suites, and real unpacked public CLI graph acceptance run serially by group.
Public acceptance checks both executable choices, all seven languages, test switches,
original source/local calls, empty selection and read-only input; Rust covers all
three dependency scopes. Per-test inventory is
[semantic-acceptance.json](../resources/semantic-acceptance.json).

Only successful complete clean-source acceptance emits
`verification-<target>.json`. Focused/local passes never stand in for this evidence.
Failed/cancelled jobs upload no release artifacts. `Complete CI` requires both
platform jobs. No manual quality-gate workflow exists or is dispatched here.

## Release operation after approval and merge

A maintainer runs `.github/workflows/release.yml` from main with the exact full
`source_commit` and successful main-push `ci_run_id`. It checks repository, workflow,
branch, event, run SHA, completed/success conclusion and both exact native job names
plus `Complete CI`. It downloads the immutable artifacts from that run, checks out
that exact commit and validates every archive and the two complete per-test receipts.
No rebuild substitutes different bytes. All hashes and file limits are checked again
before the publishing step receives its write credential.

The publisher creates `v0.1.0` as a draft prerelease at that exact commit, uploads all
31 assets, reads back every byte and only then publishes with `make_latest: false`.
Existing tags at another commit, different assets, extra assets, incomplete published
releases, failed CI or partial evidence stop the operation. No `--clobber`, asset
delete or version overwrite exists. An identical already-published version is a
read-only success; a draft can only receive missing identical intended assets.
Configure the `codegraph-release` environment protection as appropriate before use.
No workflow has been dispatched and nothing has been published by this implementation.
