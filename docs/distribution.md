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
Keep incremental and each worktree's own targets. The wrapper uses `lockf` or
`flock` when available; if neither exists, it prints a warning and runs without a
repository lock. When `sccache` is absent and no custom `RUSTC_WRAPPER` is set, it
prints a warning and falls back to ordinary Cargo compilation. A custom wrapper
is preserved. Calling the wrapper alone therefore proves neither lock acquisition
nor cache use. Hosted jobs still run their build/test groups serially; GitHub's
artifact/cache restore is separate from this compiler-cache behavior.

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

Consumers can expose already-installed target SDKs through these standard entry
points before starting the public CLI; no private `NARRANT_*` variables or helper
configuration file is needed:

- Java reads `JAVA_HOME`, `JDK_HOME`, every `java` executable on `PATH`, and
  conventional JDK install directories. Put each custom installation's `bin` on
  `PATH` to discover multiple versions. `JAVA_HOME_17_X64` and similar CI-specific
  variable names are not scanned; map one to `JAVA_HOME` or add its `bin` to `PATH`.
  Discovery keeps the highest stable patch per Java major version; the project's
  declared release selects the target declarations.
- Go reads `GOROOT`, every `go` executable on `PATH`, and conventional SDK install
  directories. Multiple custom SDK `bin` directories may appear on `PATH`.
  Discovery keeps the highest stable patch per major/minor version; project
  declarations and the helper's supported semantics determine the selected target.
- C# reads `DOTNET_ROOT`, `DOTNET_ROOT_ARM64`, `DOTNET_ROOT_X64`, `DOTNET_ROOT_X86`,
  `DOTNET_ROOT(x86)`, the user's `.dotnet`, and conventional .NET install directories.
  Each root must contain installed `packs/Microsoft.NETCore.App.Ref/<version>/ref/<tfm>`
  declarations; a runtime-only install is insufficient. Discovery keeps the highest
  stable patch for each supported modern TFM and the project chooses its TFM. These
  variables locate target references; the pack still uses its own private runtime.

These entry points add discovery candidates rather than restricting discovery to
one installation. Equal versions retain the first discovered candidate. Analysis
reads installed declarations and project metadata without installing SDKs or
running project builds.

Full Rust acceptance additionally requires `NARRANT_PROJECT_RUST_TOOLCHAIN=stable`,
an isolated absolute `CARGO_HOME`, stable `rust-src` and historical 1.81.0, 1.94.0,
1.96.0 toolchains; run `prepare-rust-native-tests.mjs --prepare` to prepare only
locked test metadata and verify its offline availability. These fixture versions
are not a product target-version allowlist.

`.github/workflows/ci.yml` runs on hosted `macos-15` ARM64 and `windows-2022` x64.
Host architecture is independently checked by native builders. Windows runs the
shared process probe before expensive downloads, then selects exact MSVC/SDK/redist
from the unchanged pins. macOS selects Xcode 16.4 / SDK 15.5. All seven helper suites,
70 named real-pack tests on macOS (64 on Windows), including all 69/63 retained
semantic tests, the full engine and public CLI lifecycle suites, and real unpacked
public CLI graph acceptance run serially by group.
The C# factory group exercises the packaged runtime through the production factory
and standard SDK discovery; its internal assertion retains diagnostics without
changing the public protocol's redacted errors. Public acceptance continues with
later languages after a package failure, then reports all original failures and
exits unsuccessfully. No partial run emits a successful receipt.
CodeGraph also checks its private SDK packaging policy on the verified extracted
Go, Java and C# archives: `target_sdk.kind` is respectively `go`, `java`, `dotnet`,
`platforms` is empty, and `runtime/platform` is absent. This producer acceptance
retains the former install-test guarantees without making private layout a
consumer dependency.
Public acceptance checks both executable choices, all seven languages, test switches,
original source/local calls, empty selection and read-only input; Rust covers all
three dependency scopes. Java uses `src/Sample.java` with a `java-static-v1`
`narrant-java.json` declaring `src` and release 17. C/C++ uses a compilation
database bound to the actual temporary project root with `clang++`, `-std=c++20`
and the selected source. The fixed factory supplies one approved native target
SDK, so this database selects that target without guessing its deployment triple.
Both metadata files belong to the read-only snapshot. After valid analysis passes,
removing the required metadata must produce the public `analysis_failed` outcome
and exit 1; a spawn error, timeout or successful empty graph cannot pass this check.
Per-test inventory is
[semantic-acceptance.json](../resources/semantic-acceptance.json).

Archive commands use the explicit system bsdtar (`SystemRoot/System32/tar.exe` on
Windows, `/usr/bin/tar` on macOS), including Node ZIP and npm tgz inputs. They do
not select Git Bash's GNU tar from PATH; paths with drive letters and spaces are
covered by the real archive tests.

Only successful complete clean-source acceptance emits
`verification-<target>.json`. Focused/local passes never stand in for this evidence.
The shared [command specification](../scripts/language-packs/verification-commands.mjs)
defines all 17 macOS / 16 Windows groups, including their argv, working directories, environment
parameters and required semantic names. Before recording evidence, the writer
compares the actual executed command list with that full specification. The
receipt stores portable identities using tool/repository path placeholders; the
release validator compares the complete platform-specific list exactly. Missing,
replaced or duplicate commands cannot substitute for any helper, engine, CLI or
public acceptance group. These receipt identities are internal CI metadata, not
an executable configuration or a consumer pack contract.
The platform lists follow the original test `cfg`: seven shell/file-symlink probe
cases exist only on Unix, and one Python junction-escape case exists only on
Windows. The Unix-only production-probe group is not sent to Windows as a zero-test
command. Windows still runs real helper cancellation, directory alias ownership,
junction escape, shared process Job tests and public CLI cancellation/parent-kill
acceptance. Junction command output is captured for assertion diagnostics so it
cannot split libtest's required named-success lines. Zero-test Cargo groups and
missing named results remain failures.
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
The final draft read rechecks its release ID, tag, exact target commit and prerelease
identity. The publication PATCH also pins that tag and target commit, while the
existing-tag commit checks remain mandatory before and after publication.
No workflow has been dispatched and nothing has been published by this implementation.
