# CodeGraph analyzer protocol 1

This is the direct language-program contract confirmed on 2026-10-22. It supersedes the unreleased ready/cancel transport. The protocol remains `codegraph.analyzer/1`, with engine/package version `0.1.0`; no released version or legacy mode exists.

## Entry and ownership

Each trusted package exposes `bin/codegraph-<id>[.exe] analyze`, with fixed IDs `rust`, `typescript-javascript`, `python`, `java`, `go`, `csharp`, `c-cpp`. `--help`, `analyze --help` and `--version` print text and exit; unknown or repeated CLI arguments, including old pack selectors and `__worker`, exit 2.

The language program itself parses the public request, discovers target SDKs, analyzes and emits the final Code Graph. There is no cross-language Rust protocol host, private engine message hop or analysis worker. TS/Java launchers only locate bundled runtimes, inherit standard handles and propagate exit status; they do not inspect JSON or perform analysis.

The caller verifies and installs the immutable package, chooses the language, preserves the source state and owns process-tree termination for cancellation, deadlines and transport failures. Runtime dependencies travel with the package. Target SDK discovery does not install or upgrade tools or run project build scripts; Rust retains its explicit all-dependencies network policy and disables project build scripts/procedural macros.

## One input, one final output

Stdin contains exactly one UTF-8 JSON object, followed by EOF. A trailing newline is optional; ordinary JSON whitespace is allowed. A second JSON value or trailing non-whitespace is invalid. The caller writes the request, flushes and closes stdin. EOF after a complete request is normal input completion, never cancellation.

Stdout contains exactly one final JSON object followed by a newline, then the program exits. There are no ready, cancel, progress or cancelled messages. Stderr is diagnostic only. The caller must read/drain stdout and stderr and enforce resource/deadline limits without pipe deadlocks.

```json
{"protocol":"codegraph.analyzer/1","type":"analyze","id":"example-1","repository_root":"/absolute/source","files":[],"options":{"include_tests":false,"rust_dependency_scope":"repository-only"}}
```

All fields except `files` are required. Unknown and duplicate fields are invalid; duplicate object keys are rejected recursively. Requests have bounded nesting. IDs are nonblank strings of at most 1,024 UTF-8 bytes without NUL. `repository_root` is an existing absolute directory without NUL. Selection paths are unique, nonempty repository-relative POSIX paths; absolute paths, backslashes, colons, NUL, empty components, `.` and `..` are invalid. Canonical paths cannot escape the source root; source and analyzer installation must not overlap.

`include_tests` is boolean. `rust_dependency_scope` is `repository-only`, `standard-library` or `all-dependencies`; non-Rust analyzers ignore this required Rust-only option. Requests cannot inject commands, execution environment, compiler flags, SDK roots or internal catalogs.

- Omitted `files`: use the language's configured project source discovery.
- `files: []`: valid empty graph, without project configuration or SDK discovery.
- Nonempty `files`: exact graph membership; other source may provide context or external symbols.
- `null`, duplicates and malformed paths are invalid.

Discovery uses native project configuration and existing read-only policy, not a recursive extension scan. Valid configured zero-source projects may succeed empty; missing or invalid required configuration fails. C/C++ retains its compilation-database requirements. See [selection policies](engine-selection.md).

## Final identity and result

Every final result carries compiled identity: `protocol`, `engine_version`, `pack_id`. This replaces the removed ready message. The caller checks all three against its fixed package before using the result. An invalid request cannot choose or overwrite this identity.

```json
{"protocol":"codegraph.analyzer/1","engine_version":"0.1.0","pack_id":"rust","type":"completed","id":"example-1","outcome":"ok","graph":{"root_contains":[],"nodes":[],"external_symbols":[]}}
```

Success exits 0. A handled failure has no graph and exits 1:

```json
{"protocol":"codegraph.analyzer/1","engine_version":"0.1.0","pack_id":"rust","type":"completed","id":"example-1","outcome":"error","error":{"code":"analysis_failed","message":"源码分析失败"}}
```

When no valid request ID was accepted, `id` is null. Stable error codes remain `invalid_request`, `incompatible_protocol`, `invalid_input`, `invalid_pack`, `analysis_failed`, `resource_limit`, `timeout`, `io_failure`, `cleanup_failed`, `internal_failure`; messages may evolve. No graph is attached to errors. Normal unresolved symbols are valid graph data, not whole-analysis failure.

Startup/transport failure, crash or forced termination may produce no final JSON. EOF alone, a result without successful exit, or successful exit without one complete matching result is never success. CLI usage exits 2; cancellation by killing the process has platform-specific status and does not promise an error JSON.

## Graph

`graph` has exactly `root_contains`, `nodes`, `external_symbols`. Existing graph semantics remain unchanged:

| Field | Contract |
| --- | --- |
| category | container, type, callable, value, execution_region, extension |
| id, language, native_kind | Strings; stable language-namespaced identity independent of checkout absolute location |
| name, signature, code, source | Required, string or null; preserve normalized self-content and display source |
| contains | Immediate child IDs |
| references | Objects with semantic and exclusive tagged target |
| extensions | Namespaced semantic keys and arbitrary bounded JSON values |

Targets are `{ "kind":"node", "id":"..." }`, `{ "kind":"external", "symbol":"..." }`, or `{ "kind":"unresolved", "name":"..." }`. External descriptors are `{ "symbol":"...", "name":"...", "scope":"repository" | "dependency" }`.

The producer ensures necessary graph invariants: unique IDs, valid containment/reachability, local reference targets and matching external descriptors; complete nodes belong to the selection. Check these once at the natural construction/output boundary, not through an extra validation process or automatic repair layer. Private source evidence, catalogs and scratch paths are not emitted. Consumers still safely decode and validate data they use across the process boundary.

## Limits and cancellation

Request and result are each at most 128 MiB; selection is at most 100,000 files; public control files remain at most 1 MiB. The caller bounds/drains stderr (256 KiB), enforces its analysis deadline (Narrant default 30 minutes), closes stdin after input and terminates the entire process tree on cancel/timeout/transport failure. Language-internal file/read limits and required native resource constraints remain in force.

Do not restore a dedicated supervisor, parent control pipe, cooperative cancel thread or extra analysis worker to implement these limits. Tools actually needed for analysis, including Rust Cargo/rustc queries, remain allowed; owned children and pipes must finish before caller leases are released. Killing only a root PID does not guarantee tree cleanup. A thin launcher can exec on macOS or spawn/wait on Windows; neither changes the JSON contract.

Examples and real seven-package acceptance must use this one-shot contract and final identity on both supported platforms. Legacy helper component fixtures may remain in tests, but production entries must not accept the old private protocol or catalogs.
