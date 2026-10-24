# 六种语言分析器的行为保持重构

2026-10-24，用户选择架构报告中的六项全部推进，并要求使用 subagent，只修改 CodeGraph、保持现有行为并运行相关测试。Clang 已完成，本计划不再修改它。

## Spec and design

本计划以用户已确认的范围、`GLOSSARY.md`、`docs/adr/0001-implement-public-protocol-in-native-analyzers.md`、`docs/protocol.md` 及各语言现有行为为依据。架构报告是候选说明，不能覆盖这些要求。

每种语言深化现有 module 的职责归属，通过小而完整的 interface 隐藏协同状态；不按文件行数拆分，不为单个调用方增加通用 adapter。内部命名和函数签名是可逆实现选择，不再逐项要求用户确认。

## Global Constraints

- Work only in `/Users/lizhanfa/projects/CodeGraph` on `codex/issue-205-standalone-engine`; keep the current checkout and caches. Do not edit or test Narrant. Do not modify `analyzers/clang/`.
- Each task owns only its named language directory. Shared `analyzers/common/`, integration tests, distribution scripts, dependency pins, CI, release metadata and the glossary/ADR are outside the write scope; report a concrete need before changing them.
- Preserve public `codegraph.analyzer/1`, version/pack identity, one JSON request plus EOF and one final JSON then exit, CLI usage and exit status, caller process-tree cancellation, packaging shape and fixed runtime versions. Add no host, worker, server, protocol hop or installer.
- Preserve Code Graph node IDs, selection/coverage/origin, selected files versus semantic context, configuration union, external references, test filtering, evidence, limits, read-only behavior, genuine Project Target SDK discovery/validation and snapshot consistency. `files: []` must retain its short circuit. Preserve deliberate public-versus-semantic-fixture differences in checks, evidence and detailed errors.
- Remove only demonstrated redundant internal coordination. Do not delete real path, SDK, provenance, graph or resource checks merely because an old-shaped in-process DTO is removed. Unresolved symbols remain legal results. Do not introduce graph repair or broader defensive checks.
- Add readable Chinese comments to new or meaningfully changed methods, structs and classes. Keep tests separate from production files; Rust private tests belong in module `tests/` directories. Follow the language's existing style.
- Use `scripts/cargo-agent.sh` for every Cargo invocation, retain incremental compilation and existing per-checkout targets, and never launch concurrent cold Rust builds. Reuse prepared fixed dependencies; do not auto-install project SDKs, change default toolchains or clean unrelated artifacts.
- Before editing, run the relevant existing baseline tests; after editing, run focused checks and one affected-language suite. Existing meaningful behavior tests are preferred; add regression tests only for uncovered responsibilities affected by the refactor. Do not add tests that merely mirror file layout or implementation. Record exact commands, environment assumptions, output, skips and source identity in this plan's report/workspace. Never claim Windows validation from a macOS run or old packaged executables.
- Do all work for a task yourself; do not spawn subagents or reviewers. Self-review your diff, then create a local commit containing only your task. No push, merge, release or other external writes. Use only this plan's workspace for reports and logs; leave sibling plan workspaces untouched.

## Execution and acceptance

Controller dispatches fresh implementers and task reviewers sequentially, then a final review of this plan's full range starting at `d824e318e8b65bf87b23662f870a88583807f7f9`. Final review covers the six tasks together, not the unrelated pre-existing standalone-engine migration. Each review reports spec compliance and quality. The local commit range and test evidence remain reviewable; Windows and unavailable platform checks remain explicit limitations.

### Task 1: Rust internal analysis request

Own `analyzers/rust/` only. The public runtime currently rebuilds a protocol/type/id/input/environment/limits envelope in `src/runtime.rs`, and `src/projection/mod.rs`/`src/protocol.rs` decode or validate that in-process envelope again. Replace this redundant coordination with a typed complete-analysis input at the existing projection seam. Keep projection/loading/native database implementations deep; do not split references/loading files by size or add a generic host.

Inspect `src/runtime.rs:74`, `src/projection/mod.rs:29`, `src/protocol.rs:63`, and semantic fixtures `src/tests/projection.rs`/`src/tests/discovery.rs`. Public profile should reach the native dependency scope without a string round trip. Remove synthesized scratch/limit or envelope fields only where they genuinely do not affect native analysis. Keep real scope/platform/path/source/SDK validation; in particular `src/tests/discovery.rs` contains a meaningful nonempty-platform case, so platform data is not dead.

Semantic fixtures may construct the new typed internal input, while preserving their meaningful scenarios and output assertions. Preserve all three dependency scopes, omitted/empty/null file handling, toolchain/sysroot/rust-src containment, read-only Cargo metadata, immutable snapshots, alias coverage, tests/include ownership, atomic failures and historical Cargo compatibility. Keep public protocol validation in the public adapter.

Run baseline and final Rust checks/tests using fixed build toolchain 1.98.0 and the repository Cargo wrapper. The suite runner `scripts/language-packs/run-rust-helper-test.mjs` clears injected build-toolchain variables; use the documented runner for native tests. Explicit `CODEGRAPH_PROJECT_RUST_TOOLCHAIN`, absolute prepared `CARGO_HOME`, rust-src and historical toolchains are prerequisites, not runtime installation work. Start from `analyzers/rust/README.md` and `scripts/language-packs/verification-commands.mjs`. Run check --lib and the full affected library/initial_protocol/lifecycle suite once after changes; classify unavailable prerequisite coverage precisely. Keep all evidence under this task's report directory.

### Task 2: Python project preparation and snapshots

Own `analyzers/python/` only. Deepen the existing project preparation module so callers no longer join the public `Discovery.files/projects/snapshots/owners/policies` tables themselves. Inspect `src/projection/discovery.rs:15`, `src/projection/mod.rs:85`, `:142`, `:232`, and `src/projection/references.rs:513`. Provide complete operations for prepared-file ownership, per-project/per-version analysis context and snapshot verification; hide coupled state behind that seam. Avoid a set of trivial getters or another wrapper around the same caller coordination.

Preserve PythonProject's native database and frozen System, nested excludes and own roots, extra paths, PEP 723, explicit-versus-discovered configuration ownership, cross-project target Python versions, common frozen source snapshots, context-versus-output selection, .py/.pyi behavior, vendored provenance, fixed Ruff/ty revision, root checks, limits and detailed failures. Do not bundle the secondary request-DTO cleanup or arbitrarily split syntax/references/bindings.

Read `analyzers/python/README.md`; run baseline/final Rust checks through the wrapper with 1.98.0. Final scope includes check --lib plus library, initial_protocol and lifecycle tests with --test-threads=1. This native implementation should not need a Python interpreter or external stdlib for semantic tests. Adapt existing discovery/reference tests through the complete module seam, retaining behavioral assertions rather than exposing replacement tables.

### Task 3: Go project planning and source admission

Own `analyzers/go/` only. Deepen project planning so a complete prepared analysis owns source admission, module/replacement/configuration coverage and the ordering that populates local-replacement facts. Inspect `analyze.go:990-1094`, `project.go:676`, `analyze.go:112`, `analyze.go:1061`, `project.go:395-415` and `project.go:666`. The existing temporal coupling is architectural friction, not evidence of an existing functional bug.

Move the actual coordinated operation behind a private plan seam; do not merely relocate the long function or add pass-through files. Keep the real go/types importer adapter and existing SDK catalog/build-tag readers. Do not change public-versus-fixture success/evidence validation as part of this task.

Preserve independent-module isolation, local replacement precedence, per-package language versions, exact selected-versus-context graph membership, SDK configuration unions from imported and unselected sources, active/excluded-test roles, cancellation, read-only behavior and evidence. Add a focused behavioral regression only if existing coverage does not exercise local-replacement/imported-context ordering.

Use the prepared Go SDK with GOROOT set to that SDK, GOTOOLCHAIN=local, GOWORK=off, GOPROXY=off and GOSUMDB=off. Run the relevant discovery/workspace/imported-context baseline, focused tests during work and `go test ./...` after changes; build the current-source analyzer and run the Python CLI/fixture suite using its explicit analyzer path. Cross-minor target SDK skips must be stated; no old installed analyzer may stand in for the changed source.

### Task 4: TypeScript/JavaScript runtime ownership

Own `analyzers/typescript/` only. Consolidate fixed Analyzer Runtime loading and verification so main/analyze/tests do not reconstruct compiler roots, package versions and catalogs independently. Inspect `runtime.mjs:7`, `main.mjs:68-72`, `analyze.mjs:17-27`, and `tests/helper.test.mjs:179`. Use a small complete runtime operation at the existing analysis seam. The in-process request envelope is not itself a public protocol defect; simplify only the demonstrated runtime knowledge leakage. Do not introduce an arbitrary compiler adapter or start the secondary workspace/session refactor.

Preserve early empty-file handling, authorization before excluded-file filtering, failure behavior for irrelevant malformed configs, frozen snapshots, original JSON AST versus sanitized configuration overlays, canonical aliases and lexical origins, per-configuration AST/binding ownership, type-only routing and unresolved conflicts, graph checks/evidence, and one-shot CLI/thin-launcher behavior. Preserve pinned TypeScript 6.0.3, @types/node 22.20.1 and undici-types 6.21.0.

Use the existing extracted exact-version runtime only as dependency input for current source. Top-level .mjs files are staged automatically; keep the change compatible with this layout rather than silently requiring shared staging edits. Run the baseline runtime/config/helper tests and the full twelve-file Node test suite from verification-commands.mjs once after changes. Tests may share real runtime preparation but must still exercise malformed/mismatched runtime behavior at the appropriate seam.

### Task 5: Java project selection/configuration ownership

Own `analyzers/java/` only. Deepen ProjectModel so Analyzer does not repeatedly reconstruct selection/activity/parser-root/configuration facts from package-visible `repository/configurations/selected/files/includeTests`. Inspect `src/dev/codegraph/java/ProjectModel.java:34-38`, `Analyzer.java:151`, `:187`, `:210`, and `:640`. Move complete model operations and the state they interpret together, reduce caller coordination, and keep the existing SdkDiscovery.Provider production/fixture seam. Do not split Analyzer by AST kinds or bundle the secondary JDK filesystem-facts refactor.

Preserve selected-versus-semantic-context membership, overlapping main/test roots, configuration union and excluded-test evidence fallback, release/preview rules, isolated JDT binding indexes, unresolved facts across configuration unions, repository externals, SDK/source disjointness, read-only behavior, public JSON/errors, empty-before-config semantics, and fixture evidence/detailed failures. Keep discovery skip-unsuitable behavior distinct from strict candidate binding.

Build current sources and test classes via `build.py ... --tests` with the fixed JDK 25 runtime and locked JARs. Run relevant discovery/target-version/public/component baseline cases then the full Java Python unittest suite once. Use real existing JDK 17/25 and older target inputs where prepared; distinguish missing prerequisites/skips. No previous installed binary is acceptance for changed classes.

### Task 6: C# static configuration policy and reference identity

Own `analyzers/csharp/` only. Deepen the static configuration module so provenance paths are not decoded as policies by Analyzer. Inspect `Configuration.cs:9-12`, `:91`, `Analyzer.cs:116`, `:122`, and `:162`. Own source-set versus SDK-project behavior and project-reference identity where configurations are constructed; expose meaningful complete facts/operations, not an extracted filename predicate or eager bag of every future policy.

Preserve explicit source-set constants, SDK implicit constants only after validated reference matching, numeric language defaults/failures, top-level source-set output, Debug/Release inherited reference contexts, cycles and excluded-test failures, exact TFM/patch/configuration selection, and read-only operation. In particular preserve pathological-TFM rejection before implicit symbol expansion.

Preserve the intentional adapter distinction: Program validates completed evidence and maps public error codes; ComponentDriver preserves raw evidence and detailed errors/paths. Do not move production OutputChecks.Sources into shared Analyzer.Analyze. GraphAccumulator is already deep and outside this candidate. Both production and component projects compile root .cs sources; if relocating files, update both source lists within this language directory.

Use fixed prepared .NET SDK 10.0.100. Build production and component projects, run focused inherited-reference/version/pathological-target baseline cases and the full C# Python unittest suite once. Supply real net8/net10 reference roots and net9 9.0.20 when available, current-source analyzer apphost and explicit runtime/target SDK variables. Record unavailable-platform checks; keep both target RIDs and apphost layout unchanged.
