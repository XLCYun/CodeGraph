# CodeGraph analyzer protocol 1

Each language package provides its own fixed public analysis entry:

```text
<package-root>/bin/codegraph-<id>[.exe] analyze
```

IDs: `rust`, `typescript-javascript`, `python`, `java`, `go`, `csharp`, `c-cpp`.
`--help`, `analyze --help`, and `--version` print text and exit without starting a
protocol session. Unknown/repeated CLI options, including the old `--pack` and `--pack-root` selectors, exit 2.
The caller chooses and verifies a trusted immutable package. The entry locates its
package from the canonical executable location, independent of cwd or its file name,
and verifies its compiled identity against the [descriptor and private runtime layout](pack-layout.md). A request cannot select
commands, execution environment, compiler flags, or SDK roots.

## Transport and lifecycle

Stdin and stdout carry UTF-8 NDJSON: exactly one JSON object per newline-terminated
line. Strings escape embedded newlines. Stdout contains protocol only; stderr is
reserved for bounded diagnostics. One process handles one analyze request. The
protocol is `codegraph.analyzer/1`; engine and initial package versions are `0.1.0`.
The private `codegraph.engine/1` protocol is an implementation detail.

The CLI first writes:

```json
{"protocol":"codegraph.analyzer/1","type":"ready","engine_version":"0.1.0","pack_id":"rust"}
```

`ready` identifies the compiled analyzer and its fixed pack ID; package loading
and descriptor validation complete inside the supervised worker.
Consumers verify all three identities before trusting a result. They then send one
request (the root below is illustrative; the executable example creates real paths):

```json
{"protocol":"codegraph.analyzer/1","type":"analyze","id":"example-1","repository_root":"/absolute/source","files":[],"options":{"include_tests":false,"rust_dependency_scope":"repository-only"}}
```

Every field except `files` is required. Omitted `files` discovers source files from
the language's loaded project configuration. An explicit `[]` returns an empty
graph without project or SDK discovery; a nonempty array is an exact selection.
`null` and malformed selections are invalid. IDs are non-blank UTF-8 strings of at most 1,024 bytes,
without NUL. `repository_root` is an existing absolute directory. `files` are unique,
non-empty repository-relative POSIX paths; absolute paths, backslashes, colons,
empty components, `.` and `..` are rejected. Canonical file provenance is checked
by the Host; links cannot escape the repository. The source directory and package
must not overlap. The caller must keep the source state consistent during analysis.

`include_tests` is boolean. `rust_dependency_scope` is one of `repository-only`,
`standard-library`, `all-dependencies`, and is required for every pack; non-Rust
packs ignore this Rust-only option. Rust preserves its existing offline / standard
library / explicit all-dependencies policies. SDK discovery never installs or
upgrades a target toolchain. The existing explicit Rust all-dependencies policy
retains the caller's network policy; it does not enable project build scripts or
procedural macros.

The caller may close stdin after the complete request line. EOF before any request,
EOF with an unterminated line, malformed JSON, wrong protocol, unexpected messages,
unknown fields, and duplicate fields are errors. Duplicate keys are rejected
recursively, including nested options and internal graph extensions. The JSON
decoder's bounded nesting limit remains enabled. No second analyze is accepted.

While the request runs, the caller may send:

```json
{"protocol":"codegraph.analyzer/1","type":"cancel","id":"example-1"}
```

Unknown IDs, cancellation before analyze, and repeated cancel messages fail.
Cancellation applies to SDK discovery, factory loading, helper analysis, and cleanup.
An observed analysis/protocol failure is not turned into successful cancellation.
The CLI exits after one `completed`. Exit codes: 0 for `ok`, 130 for `cancelled`,
1 for protocol/analysis/transport failures, 2 for CLI usage. A transport failure may
prevent the terminal line from being delivered; EOF or exit alone is never success.

## Completed and graph

A valid empty selection returns:

```json
{"protocol":"codegraph.analyzer/1","type":"completed","id":"example-1","outcome":"ok","graph":{"root_contains":[],"nodes":[],"external_symbols":[]}}
```

An empty selection validates the public package identity and repository boundary,
but does not prepare SDKs or launch a helper. It is distinct from an analysis error.
For discovered or explicit non-empty selections the complete factory, input, source-evidence, graph, and
process-tree exit checks all precede successful output.
Valid configured zero-source discovery is also successful, after validating the
language's project configuration. Missing or invalid required configuration is an
analysis error. C/C++ requires a nonempty compilation database even for an empty
translation unit. Configuration-free TS/JS and Python projects retain their native
defaults; see [discovery policies](engine-selection.md).

`graph` has exactly `root_contains`, `nodes`, and `external_symbols`. Nodes retain
the helper graph fields:

| Field | Value |
| --- | --- |
| `category` | `container`, `type`, `callable`, `value`, `execution_region`, `extension` |
| `id`, `language`, `native_kind` | Strings; stable ID with language namespace |
| `name`, `signature`, `code`, `source` | Required fields, string or null |
| `contains` | Array of immediate child IDs |
| `references` | Array of `{ "semantic": "code.calls", "target": ... }` |
| `extensions` | Object of namespaced semantic keys and arbitrary JSON values |

Targets are exclusive tagged objects: `{ "kind":"node", "id":"..." }`,
`{ "kind":"external", "symbol":"..." }`, or
`{ "kind":"unresolved", "name":"..." }`. External descriptors are
`{ "symbol":"...", "name":"...", "scope":"repository" | "dependency" }`.
Containment has one parent per node and no cycles; every node is reachable from the
virtual root; local references target existing nodes; referenced external symbols
have one matching descriptor. Unknown semantic extension values remain preserved.
`source` is the original display source; `signature` and `code` retain normalized
self-content. No graph diff is exposed.

Private coverage/origin evidence, scratch paths and SDK catalogs are not serialized.
The same source and semantic extensions remain intact. Consumers must still strictly
deserialize the returned graph and validate their own constructed graph at this
process boundary; a successful subprocess is not a substitute for boundary validation.

Errors have no graph:

```json
{"protocol":"codegraph.analyzer/1","type":"completed","id":"example-1","outcome":"error","error":{"code":"invalid_pack","message":"支持包 ID、版本或协议与 CodeGraph 不匹配"}}
```

Codes are stable: `invalid_request`, `incompatible_protocol`, `invalid_input`,
`invalid_pack`, `analysis_failed`, `resource_limit`, `timeout`, `io_failure`,
`cleanup_failed`, `internal_failure`. Text may evolve. Analysis messages deliberately
summarize the failing stage rather than expose internal compiler paths. When no
accepted analyze ID exists, error `id` is null. Cancellation has no error or graph:

```json
{"protocol":"codegraph.analyzer/1","type":"completed","id":"example-1","outcome":"cancelled"}
```

## Limits and ownership

| Resource or stage | Limit |
| --- | --- |
| Request / result line (excluding final newline) | 128 MiB each |
| Helper stderr | 256 KiB total |
| Public package control file | 1 MiB |
| Explicit or discovered selection | 100,000 files |
| Wait for initial request / helper ready | 60 seconds each |
| Complete analysis, including factory / SDK work | 30 minutes total |
| Cooperative cancellation | 2 seconds |
| Cleanup / process exit / blocked output | 5 seconds |

Bounded queues and interruptible pipe workers prevent stdin/stdout backpressure from
holding the main lifecycle loop. A private worker isolates factory/SDK work so its
threads can be terminated when cooperation fails. The public process retains the
worker control pipe after consumer EOF; loss of that pipe means the public parent
died and triggers cancellation/cleanup. Normal cancellation gives the helper two
seconds, then up to five seconds for cleanup; the public owner can forcibly reclaim
the worker domain when necessary.

Windows uses nested kill-on-close Jobs and retained process handles. macOS uses
separate owned process groups plus observed descendant PID/birth-time snapshots,
including descendants that later change groups. Parent-kill tests cover public CLI
group termination, nested helper groups and an observed detached descendant. This
is ownership for trusted helpers, not a sandbox against arbitrary malicious
instantaneous double-fork/daemonization. Consumers retain their own process-tree
owner and must forcibly reclaim it if the deadline or transport fails.

Run the checked example with:

```sh
python3 scripts/examples/empty-selection.py target/debug/codegraph-rust
python3 scripts/examples/analyze-directory.py /verified/pack/bin/codegraph-rust /absolute/project
```

The first example creates only an empty-selection fixture. The second requires a
complete, previously verified package and a real project; it exercises directory
discovery without private runtime arguments and permits normal stdin EOF.

The distribution pipeline now includes real seven-pack public CLI acceptance on
both supported CI targets. No published artifacts or successful remote CI runs are
claimed by this local implementation; see [distribution.md](distribution.md).
