# Direct-program test migration

Baseline `ff7e648ecace3aeca672064bc3eb6ca37ffb729e` contains 42 root test/fixture files and 183 recognizable names. This is a source inventory, not an executable-test count. Fixture functions remain frozen analysis input.

Each semantic assertion uses real direct public programs or current in-process language components. The test-only public graph view deserializes/indexes original fields and never regenerates, repairs, rewrites IDs/content/edges or enlarges selections. No private Host transport or production fallback remains.

The [machine-readable map](../resources/direct-test-migration.json) includes every recognized name, disposition, destination and runnable command. [Semantic acceptance](../resources/semantic-acceptance.json) binds current names/source/platforms/commands while retaining historical names. Exact local results and remaining Windows gates are evidence outside this source map.

| Historical root family | Disposition | Current coverage / command |
| --- | --- | --- |
| `src/adapters/rust/tests/production.rs` (2 names) | native-covered | `analyzers/common/src/tests/initial.rs`<br>`analyzers/integration-tests/src/tests/rust_graph.rs`<br>Commands `rust`, `integration`, `public` |
| `src/adapters/rust/tests/toolchain.rs` (2 names) | native-covered | `analyzers/rust/src/tests/loading.rs`<br>`analyzers/integration-tests/src/tests/rust_graph.rs`<br>Commands `rust`, `integration` |
| `src/adapters/tests/csharp_graph.rs` (1 names) | moved | `analyzers/integration-tests/src/tests/csharp_graph.rs`<br>Commands `integration` |
| `src/adapters/tests/factory.rs` (5 names) | native-covered | `launchers/src/tests/startup.rs`<br>`analyzers/common/src/tests/public_protocol.rs`<br>`scripts/tests/analyzer-package-policy.test.mjs`<br>`scripts/tests/analyzer-entries.test.mjs`<br>Commands `scripts`, `common`, `public` |
| `src/adapters/tests/fixtures/typescript-historical-identity.json` (0 names) | preserved-fixture | `analyzers/integration-tests/src/tests/fixtures/typescript-historical-identity.json`<br>`analyzers/typescript/tests/fixtures/typescript-historical-identity.json`<br>Commands `integration`, `typescript` |
| `src/adapters/tests/platforms.rs` (4 names) | native-covered | `analyzers/go/public_test.go`<br>`analyzers/java/tests/java/dev/codegraph/java/SdkDiscoveryTest.java`<br>`analyzers/csharp/tests/SdkDiscoveryTests.cs`<br>`analyzers/clang/tests/sdk_discovery.hpp`<br>Commands `go`, `java`, `csharp`, `clang` |
| `src/adapters/tests/platforms/go.rs` (2 names) | native-covered | `analyzers/go/public_test.go`<br>`analyzers/java/tests/java/dev/codegraph/java/SdkDiscoveryTest.java`<br>`analyzers/csharp/tests/SdkDiscoveryTests.cs`<br>`analyzers/clang/tests/sdk_discovery.hpp`<br>Commands `go`, `java`, `csharp`, `clang` |
| `src/adapters/tests/platforms/java.rs` (2 names) | native-covered | `analyzers/go/public_test.go`<br>`analyzers/java/tests/java/dev/codegraph/java/SdkDiscoveryTest.java`<br>`analyzers/csharp/tests/SdkDiscoveryTests.cs`<br>`analyzers/clang/tests/sdk_discovery.hpp`<br>Commands `go`, `java`, `csharp`, `clang` |
| `src/adapters/tests/typescript_graph.rs` (23 names) | moved | `analyzers/integration-tests/src/tests/typescript_graph.rs`<br>Commands `integration` |
| `src/child_process/tests/fixture.rs` (1 names) | moved-test-utility | `analyzers/windows-process-probe/src/child_process/tests/fixture.rs`<br>Commands `process` |
| `src/child_process/tests/process_tree.rs` (6 names) | moved-test-utility | `analyzers/windows-process-probe/src/child_process/tests/process_tree.rs`<br>Commands `process` |
| `src/child_process/tests/support.rs` (0 names) | moved-test-utility | `analyzers/windows-process-probe/src/child_process/tests/support.rs`<br>Commands `process` |
| `src/child_process/tests/unix_group.rs` (1 names) | moved-test-utility | `analyzers/windows-process-probe/src/child_process/tests/unix_group.rs`<br>Commands `process` |
| `src/child_process/tests/windows_job.rs` (8 names) | moved-test-utility | `analyzers/windows-process-probe/src/child_process/tests/windows_job.rs`<br>Commands `process` |
| `src/child_process/tests/windows_support.rs` (0 names) | moved-test-utility | `analyzers/windows-process-probe/src/child_process/tests/windows_support.rs`<br>Commands `process` |
| `src/child_process/tests/windows_tracking.rs` (4 names) | moved-test-utility | `analyzers/windows-process-probe/src/child_process/tests/windows_tracking.rs`<br>Commands `process` |
| `src/child_process/tests/windows_validation.rs` (0 names) | moved-test-utility | `analyzers/windows-process-probe/src/child_process/tests/windows_validation.rs`<br>Commands `process` |
| `src/cli/tests/protocol.rs` (4 names) | native-covered | `analyzers/common/src/tests/initial.rs`<br>`analyzers/common/src/tests/public_protocol.rs`<br>`scripts/tests/public-cli-transport.test.mjs`<br>Commands `common`, `scripts`, `public` |
| `src/cli/tests/transport.rs` (1 names) | retired-control-only | `scripts/tests/public-cli-transport.test.mjs`<br>`analyzers/common/src/tests/lifecycle.rs`<br>Commands `scripts`, `rust` |
| `src/code_graph/tests/fixtures.rs` (0 names) | native-covered-fixture | `analyzers/common/src/tests/historical_graph.rs`<br>Commands `common` |
| `src/code_graph/tests/graph.rs` (18 names) | native-covered | `analyzers/common/src/tests/historical_graph.rs`<br>`analyzers/common/src/tests/graph.rs`<br>Commands `common` |
| `src/code_graph/tests/mod.rs` (0 names) | native-covered-fixture | `analyzers/common/src/tests/historical_graph.rs`<br>Commands `common` |
| `src/native_analyzer/tests/directory/clang.rs` (1 names) | moved | `analyzers/integration-tests/src/tests/directory/clang.rs`<br>Commands `integration` |
| `src/native_analyzer/tests/directory/csharp.rs` (1 names) | moved | `analyzers/integration-tests/src/tests/directory/csharp.rs`<br>Commands `integration` |
| `src/native_analyzer/tests/directory/go.rs` (1 names) | moved | `analyzers/integration-tests/src/tests/directory/go.rs`<br>Commands `integration` |
| `src/native_analyzer/tests/directory/java.rs` (1 names) | moved | `analyzers/integration-tests/src/tests/directory/java.rs`<br>Commands `integration` |
| `src/native_analyzer/tests/directory/mod.rs` (0 names) | moved | `analyzers/integration-tests/src/tests/directory/mod.rs`<br>Commands `integration` |
| `src/native_analyzer/tests/discovery.rs` (6 names) | native-covered | `analyzers/common/src/tests/graph.rs`<br>`analyzers/rust/src/tests/discovery.rs`<br>`analyzers/python/src/projection/tests/discovery.rs`<br>`analyzers/typescript/tests/workspace.test.mjs`<br>`analyzers/integration-tests/src/tests/directory`<br>Commands `common`, `rust`, `typescript`, `integration`, `public` |
| `src/native_analyzer/tests/fake-helper.mjs` (0 names) | native-covered | `analyzers/common/src/tests/graph.rs`<br>`analyzers/common/src/tests/initial.rs`<br>`analyzers/common/src/tests/lifecycle.rs`<br>`analyzers/windows-process-probe/src/child_process/tests`<br>`scripts/tests/public-cli-transport.test.mjs`<br>Commands `common`, `scripts`, `rust`, `public`, `process` |
| `src/native_analyzer/tests/fixtures/pr31_github_discovery.rs` (4 names) | preserved-fixture | `analyzers/integration-tests/src/tests/fixtures/pr31_github_discovery.rs`<br>Commands `integration` |
| `src/native_analyzer/tests/fixtures/python-historical/bindings.json` (0 names) | preserved-fixture | `analyzers/integration-tests/src/tests/fixtures/python-historical/bindings.json`<br>Commands `integration` |
| `src/native_analyzer/tests/fixtures/python-historical/projects.json` (0 names) | preserved-fixture | `analyzers/integration-tests/src/tests/fixtures/python-historical/projects.json`<br>Commands `integration` |
| `src/native_analyzer/tests/fixtures/python-historical/relationships.json` (0 names) | preserved-fixture | `analyzers/integration-tests/src/tests/fixtures/python-historical/relationships.json`<br>Commands `integration` |
| `src/native_analyzer/tests/migration.rs` (9 names) | native-covered | `analyzers/rust/src/tests/runtime.rs`<br>`analyzers/rust/src/tests/loading.rs`<br>`analyzers/python/src/tests/core.rs`<br>`analyzers/typescript/tests/public.test.mjs`<br>`analyzers/common/src/tests/public_protocol.rs`<br>Commands `rust`, `common`, `typescript`, `public` |
| `src/native_analyzer/tests/mod.rs` (18 names) | native-covered; control cases retired individually | `analyzers/common/src/tests/historical_evidence.rs`<br>`scripts/tests/public-cli-transport.test.mjs`<br>`analyzers/windows-process-probe/src/child_process/tests`<br>C++ native SDK/header assertions; commands `common`, `scripts`, `public`, `process`, `clang`, `integration` |
| `src/native_analyzer/tests/observation.rs` (3 names) | moved | `analyzers/common/src/tests/observation.rs`<br>Commands `common` |
| `src/native_analyzer/tests/package_observation.rs` (2 names) | native-covered | `analyzers/common/src/tests/observation.rs`<br>`analyzers/common/src/tests/public_protocol.rs`<br>`scripts/tests/public-cli-observation.test.mjs`<br>Commands `common`, `scripts`, `public` |
| `src/native_analyzer/tests/python_graph.rs` (11 names) | moved | `analyzers/integration-tests/src/tests/python_graph.rs`<br>Commands `integration` |
| `src/native_analyzer/tests/python_native.rs` (22 names) | moved | `analyzers/integration-tests/src/tests/python_native.rs`<br>Commands `integration` |
| `src/native_analyzer/tests/rust_graph.rs` (14 names) | moved | `analyzers/integration-tests/src/tests/rust_graph.rs`<br>Commands `integration` |
| `src/native_analyzer/tests/rust_syntax.rs` (4 names) | moved | `analyzers/integration-tests/src/tests/rust_syntax.rs`<br>Commands `integration` |
| `src/pack/tests/location.rs` (2 names) | native-covered | `launchers/src/tests/startup.rs`<br>`scripts/tests/analyzer-package-policy.test.mjs`<br>`analyzers/common/src/tests/public_protocol.rs`<br>Commands `scripts`, `common`, `public` |

## Runnable command keys

- `integration`: `bash scripts/cargo-agent.sh test --manifest-path analyzers/integration-tests/Cargo.toml --locked --lib <filter> -- --ignored --test-threads=1`
- `common`: `bash scripts/cargo-agent.sh +1.98.0 test --manifest-path analyzers/common/Cargo.toml --locked --lib`
- `process`: `bash scripts/cargo-agent.sh test --manifest-path analyzers/windows-process-probe/Cargo.toml --locked --lib child_process:: -- --test-threads=1`
- `rust`: `bash scripts/cargo-agent.sh +1.98.0 test --manifest-path analyzers/rust/Cargo.toml --locked -- --include-ignored --test-threads=1`
- `go`: `<pinned-go>/bin/go test ./... (cwd analyzers/go)`
- `java`: `python3 -m unittest discover -s analyzers/java/tests -v`
- `csharp`: `python3 -m unittest discover -s analyzers/csharp/tests -v`
- `clang`: `python3 -m unittest discover -s analyzers/clang/tests -v`
- `typescript`: `node --test --test-concurrency=1 analyzers/typescript/tests/*.test.mjs`
- `scripts`: `pnpm test`
- `public`: `node scripts/language-packs/verify-public-cli.mjs <fresh-exact-head-output> all`

Configure the pinned SDK and actual package environment before running native tests. `verification-commands.mjs` supplies exact production CI argv/env/filter lists, rejects zero tests/missing named successes and binds the complete clean-source receipt. Local script tests use genuine child processes for transport/status failures; expected negative protocol outcomes are not suite failures.

The additional old root CLI family (`tests/cli.rs`, 16 cases) is recorded per case under `additionalFamilies` in the machine-readable map, outside the unchanged 42/183 baseline accounting: public fixed identity/empty/selection/invalid input cases now run across all seven real extracted packages; missing/malformed/nonzero/stdio regressions use `public-cli-transport.test.mjs`; active native analysis, output backpressure and input waits use native lifecycle tests; caller pre-resume Job and descendant barriers use process-probe tests. Old ready/cancel/cancelled, open-stdin completion, worker identity/count and wrong private descriptor behavior have no production counterpart. Package validity remains caller-owned and is covered by archive/manifest/descriptor/hash/path/license policy tests.

The two old generic graph tests for cross-language merging/foreign language states remain the Narrant caller’s structural/merge responsibility. The other 16 invariant cases now invoke the actual common output validator rather than a copied validator. Historical source tests, test roles, source/evidence/readonly and target SDK policies remain in their native or moved public suites.

The old mixed Host family is mapped per case rather than to a module declaration. Its Unicode/empty-source/reference cycle acceptance and all 19 graph/source rejection variants call the actual common `finish` component directly. Pure private DTO decoding (including missing nullable fields), catalog injection and ready/cancel Session shape assertions retire with those deleted boundaries; the public protocol and native component assertions listed per case remain executable.

Detailed Rust/Python error assertions read the actual bounded stderr emitted by the real program alongside the unchanged public error code/message. They do not require private error messages to reappear in public JSON. The TypeScript duplicate-selection case checks stable `invalid_request`; only the removed Host’s specific Chinese wording retires. The Python deadline/panic case waits for actual native configuration/PID evidence before timing, drains/joins pipes, and checks completed ProcessTree cleanup.
