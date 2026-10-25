# CodeGraph 0.1.1 distribution contract

CodeGraph v0.1.1 fixes Go directive detection while retaining `codegraph.analyzer/1`.
Publication requires complete successful CI on macOS ARM64 and Windows x64; every
manifest binds its archive to the exact clean source commit. The earlier
[v0.1.0 prerelease](https://github.com/XLCYun/CodeGraph/releases/tag/v0.1.0) and all
of its bytes remain immutable. Narrant application releases use a separate channel.

## Fixed assets and provenance

Repository `XLCYun/CodeGraph`, tag `v0.1.1`, package/engine version `0.1.1`, protocol
`codegraph.analyzer/1`. Targets are `aarch64-apple-darwin` and
`x86_64-pc-windows-msvc`. Each target has seven archive/manifest pairs for `rust`,
`typescript-javascript`, `python`, `java`, `go`, `csharp`, `c-cpp`.

```text
https://github.com/XLCYun/CodeGraph/releases/download/v0.1.1/<id>-<target>-pack.tar.gz
https://github.com/XLCYun/CodeGraph/releases/download/v0.1.1/<id>-<target>-manifest.json
https://github.com/XLCYun/CodeGraph/releases/download/v0.1.1/SHA256SUMS
```

One release contains 29 assets: 14 archives, 14 manifests and `SHA256SUMS` covering
the other 28. Each archive contains one direct public entry, descriptor, its runtime
resources and licenses according to [pack-layout.md](pack-layout.md).

Manifest fields are exactly `schemaVersion`, `id`, `version`, `target`,
`engineVersion`, `protocol`, full `sourceCommit`, boolean `sourceDirty`,
`archiveSize`, `archiveSha256`, and `files` (`path`, `size`, `sha256`, `executable`).
Published bytes require `sourceDirty: false` and equal engine/package versions.
Canonical compact JSON plus a newline rejects duplicate manifest keys. Limits are
256 MiB compressed, 1 GiB summed file bytes and 2 MiB manifest. Verification rejects
unsafe or case-colliding paths, device names, links, collisions, duplicate/missing/
extra entries, mode drift and byte/hash mismatches. Archives use epoch timestamps
and explicit 0755/0644 modes. Fixed source/channel trust is required; a manifest is
not a signature. Existing artifact paths are never overwritten by the builder.

## Local candidates and consumer use

Builds use `<output>/<id>/0.1.1/<target>/{manifest.json,pack.tar.gz}`. Verified extracted
copies are `<output>/smoke-installed/<id>/0.1.1/<target>`. An unrelated-cwd public
acceptance run removes all producer-only `CODEGRAPH_*` environment variables and
executes the real packaged entry. Successful clean-source all-seven acceptance
writes `candidates.json` with exactly the public six-field rows:

```json
{"id":"typescript-javascript","version":"0.1.1","engineVersion":"0.1.1","protocol":"codegraph.analyzer/1","target":"aarch64-apple-darwin","root":"/absolute/output/smoke-installed/typescript-javascript/0.1.1/aarch64-apple-darwin"}
```

`candidate-receipt.json` separately binds archive/manifest SHA256, all seven installed
file identities, exact clean source and upstream lock/pin hashes. It says
`releaseEvidence: false`: local candidates do not substitute for complete dual-platform
CI. `public-acceptance/` retains actual request, stdout, bounded stderr and exit
records, including expected errors and caller tree termination. Build a fresh output
from each committed HEAD; never relabel historical bytes.

Consumers verify fixed-tag checksums, manifest identity, bounded unpacking and each
file before atomically installing a new trusted root outside source. Keep package
and workspace leases until the root and descendants exit. Invoke only the fixed
`bin/codegraph-<id>[.exe] analyze`, write one UTF-8 JSON and close stdin. A trailing
newline is optional. Verify final protocol/engine version/pack ID/request ID/outcome,
complete graph and successful exit. Missing/malformed output, abnormal status and
forced termination cannot become success. The caller drains pipes and owns entire
process-tree termination on cancel/deadline/transport failure.

## Build and validation

Use Node 24.18.0, pnpm 11.13.1 and pinned tools. `pnpm install --frozen-lockfile`
installs only `tar@7.5.22`. The root Cargo workspace contains only `launchers`;
Rust/Python/common and non-shipped tests use their own manifests and locks. All Cargo
commands use `scripts/cargo-agent.sh`. Preserve incremental and separate worktree
targets. The wrapper warns and falls back if repository-lock tools or sccache are
unavailable; invoking it alone does not prove cache use.

```sh
pnpm test
pnpm check:scripts
scripts/cargo-agent.sh fmt --all --check
scripts/cargo-agent.sh check --locked --workspace --lib --bins
node scripts/language-packs/provision-native-toolchains.mjs /empty/isolated-tools --language all
node scripts/language-packs/verify-native-analyzers.mjs artifacts --build --language all
```

Provisioning writes fixed build and test SDK locations to `environment.json` and
`GITHUB_ENV`; local callers load those values before subsequent commands. It is a
producer build operation, never project SDK installation. Existing tools should be
reused locally. Rust/Python use Rust 1.98.0; all other build/runtime and source pins
remain in `resources/language-support-packs.json`, `resources/native-toolchains.json`,
Java's dependency lock and native manifests. C# runtime 10.0.0 RID nupkg bytes are
SHA512-verified from the official NuGet catalog before offline self-contained restore;
the full publish directory and both SDK/runtime notice variants are copied.

Target SDKs are independent of execution runtimes. Standard discovery inputs include
Java `JAVA_HOME`, `JDK_HOME`, real PATH binaries, bundled JDK and installed roots;
Go `GOROOT`, real PATH binaries and installed roots; C# `DOTNET_ROOT` variants,
user `.dotnet` and installed reference-pack roots; C/C++ approved installed macOS or
Windows/MSVC headers. The highest stable patch is selected per Java major, Go minor
or modern .NET TFM. Runtime-only .NET installs cannot provide target references.
No language installs target SDKs or runs project build scripts. Rust retains its
three dependency scopes and explicit network policy.

Complete Rust tests additionally require explicit absolute `CARGO_HOME`,
`CODEGRAPH_PROJECT_RUST_TOOLCHAIN=stable`, stable rust-src and installed historical
1.81.0/1.94.0/1.96.0 fixtures. `prepare-rust-native-tests.mjs --prepare` prepares only
locked test metadata. These versions are fixtures, not a product allowlist.

The hosted PR/main CI runs both native targets, Windows pre-resume Job/descendant
probes before tool provisioning, all native component/public suites, preserved
semantic integration names and actual seven-package public acceptance. Non-shipped
Java/C#/Clang test drivers are built separately and never copied into payloads.
The semantic inventory maps each historical name to a real source, command and
platform; a successful process missing required named-test output fails the gate.
[direct-test-migration.md](direct-test-migration.md) records each old root family.

Public package acceptance covers the shared 21 conformance vectors, no-final-newline
EOF, final compiled identity, configured discovery, exact selection and graph-outside
references, test roles, configured empty and explicit empty graphs, required/malformed
models, six graph categories and read-only inputs. Rust covers all scopes. C/C++
preserves its native test-role policy and requires a nonempty CDB. Expected negative
results require exit 1, an error and no graph. Actual loaded requests are terminated
by the caller's complete tree ownership and observed descendants must exit; Unix
process groups are not Windows evidence. Format/script tests cannot substitute for
these native runs.

Only the full exact command specification in `verification-commands.mjs`, all named
semantic successes and clean matching archive bytes can write
`verification-<target>.json`. Missing/replaced/skipped groups fail. Release validation
compares the complete platform-specific command identities and both receipts. Failed
or cancelled CI jobs upload no release artifacts. `Complete CI` requires both native
jobs; no manual quality-gate is dispatched. Release publication uses those exact CI
bytes and never rebuilds a replacement during promotion.

## Release operation after approval and merge

A maintainer runs `.github/workflows/release.yml` from main with the exact full
`source_commit` and successful main-push `ci_run_id`. It checks repository, workflow,
branch, event, run SHA, completed/success conclusion and both exact native job names
plus `Complete CI`. It downloads the immutable artifacts from that run, checks out
that exact commit and validates every archive and the two complete per-test receipts.
No rebuild substitutes different bytes. All hashes and file limits are checked again
before the publishing step receives its write credential.

The publisher creates `v0.1.1` as a draft prerelease at that exact commit, uploads all
29 assets, reads back every byte and only then publishes with `make_latest: false`.
Existing tags at another commit, different assets, extra assets, incomplete published
releases, failed CI or partial evidence stop the operation. No `--clobber`, asset
delete or version overwrite exists. An identical already-published version is a
read-only success; a draft can only receive missing identical intended assets.
Configure the `codegraph-release` environment protection as appropriate before use.
The final draft read rechecks its release ID, tag, exact target commit and prerelease
identity. The publication PATCH also pins that tag and target commit, while the
existing-tag commit checks remain mandatory before and after publication.
The earlier public `v0.1.0` release uses release ID `422619176`, source `0552cc97f217d772b6d883ba75cc9007d3fc8309`,
and the 29 exact CI assets. It remains a prerelease with `make_latest: false`.
