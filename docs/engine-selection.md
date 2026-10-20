# Private engine selection and coverage

This is the internal `codegraph.engine/1` contract used by fixed language
adapters. Consumers use `codegraph.analyzer/1` and do not assemble this request.
Both versions remain `1`, with package version `0.1.0`: the accepted standalone
analyzer specification intentionally revises the unreleased request schema so
`files` is optional. No published compatibility obligation is changed.

## Request selection

Public `files` and private `input.files` have the same three forms:

| JSON | Meaning |
| --- | --- |
| Field omitted | Discover the selection from the loaded language project |
| `"files": []` | Explicit empty selection; no project or SDK discovery |
| `"files": ["src/lib.rs"]` | Exact explicit selection; context never expands it |

`null`, scalar/object values, non-string entries, invalid relative paths and
duplicate paths are errors. A missing field must not be serialized as `null`.
The shared Rust deserializer is
`analyzers/common/src/selection.rs::deserialize_files`; the field uses
`default` plus this custom deserializer, and public serialization skips `None`.
Runtime `AnalysisInput.files` is `Option<Vec<PathBuf>>`.

The explicit-empty fast path validates the public package identity and repository
boundary, then returns a validated empty graph without preparing target SDKs,
loading native control files or starting the private engine.

Private discovery example (the environment and limits retain their existing
strict schema):

```json
{
  "protocol": "codegraph.engine/1",
  "type": "analyze",
  "id": "analysis-1",
  "input": {
    "repository_root": "/absolute/project",
    "include_tests": false,
    "profile": "rust-repository-v1"
  },
  "environment": {
    "scratch_root": "/absolute/scratch",
    "platforms": []
  },
  "limits": {
    "analysis_timeout_ms": 1800000,
    "max_result_bytes": 134217728,
    "max_diagnostic_bytes": 262144
  }
}
```

## One loaded project, one resolved selection

The language loader resolves omitted selection in the same request and immutable
project snapshot used for graph projection. It validates the required project
configuration first. It must not run a preliminary discovery process, reload the
project to project the graph, scan arbitrary extensions recursively, select every
VFS/context file, or expand selection by following references.

An explicit list retains exact coverage, including empty files and excluded test
files. A discovered list is the loader's complete configured selection. No second
`selected_files` response field exists: `evidence.files` is the authoritative
resolved selection and contains exactly one record per selected relative path.

```json
{
  "configurations": [
    {
      "id": "rust-projects",
      "source_paths": ["Cargo.toml"],
      "platform_ids": []
    }
  ],
  "files": [
    {
      "path": "src/lib.rs",
      "disposition": "analyzed",
      "configuration_ids": ["rust-projects"]
    },
    {
      "path": "checks/custom.rs",
      "disposition": "excluded_test",
      "configuration_ids": ["rust-projects"]
    }
  ],
  "origins": [
    {
      "node_id": "rust:repo:src%2Flib.rs::function:run",
      "selected_paths": ["src/lib.rs"]
    }
  ]
}
```

With `include_tests: false`, a discovered test-only file remains in coverage as
`excluded_test`, but contributes no nodes or node origins. With
`include_tests: true`, no coverage record may be `excluded_test`. Existing
`inactive` records remain valid where a language's project policy requires them;
they cannot support origins either.

Valid configuration may discover no source files. Such a successful result has an
empty graph, `files: []`, `origins: []`, and at least one actual configuration
record. A language's supported implicit/default configuration may have empty
`source_paths`; this does not permit bypassing its normal configuration loading or
validation. Missing required configuration, malformed configuration, missing SDKs
and read failures are errors, not configured-empty results.

## Host checks before graph conversion

Both explicit and discovered coverage is limited to **100,000 files**. The public
request decoder enforces the same explicit-selection limit. Language engines
must enforce the discovery limit during collection, before projection; they
retain the existing result byte, diagnostic, timeout, cancellation and cleanup
limits. The result line remains limited to 128 MiB.

For every returned coverage path the Host validates normalized repository-relative
POSIX syntax, uniqueness, canonical containment, an existing ordinary readable
file and the existing language-specific physical-alias policy. The Host does not
infer language membership from file extensions: actual compiler sources can use
`.inc`, `.def`, `.md` or no extension. Language loaders must establish source
membership and validate the source through their normal parser/project model.

Configuration IDs must be unique; their source paths must be valid readable
repository files and their platforms must be approved. Each covered file references one or
more actual configuration IDs. Explicit coverage must equal the requested set
exactly. The discovered set is trusted only after these boundary checks.

Every node has exactly one origin record, every origin has a nonempty unique list
of paths, and every origin path belongs to `analyzed` coverage. All existing
language identity, six-category, containment, reference, external-symbol and graph
checks still run afterward. No evidence is exposed in the public graph.

## Rust resolution

Rust retains the existing static-project / Cargo manifest discovery and read-only
loading policies. The loaded project records Cargo workspace members' declared
target roots, or static JSON crates marked as workspace members. A path dependency
or other VFS file is not a member merely because it is inside the repository.

Discovery walks only those loaded member crate graphs whose target roots are
inside the requested repository. A target owned by an in-repository manifest
cannot escape that boundary; sibling members of an ancestor workspace remain
context. It includes root files and actual module files,
including modules in function, associated-item, closure, constant and signature
block scopes. It uses the same native scope traversal as source completeness
validation. Declared targets must exist in the loaded crate graph; their absence
cannot become an empty success. Modules that escape the repository are rejected.
Valid empty Cargo workspaces may return configured-empty results.

Executable `include!` sources resolved in those member scopes are also selected
from the same database, with their original call locations. String/byte data,
inactive or unresolved calls and dependency-only sources remain outside selection.
This preserves the existing native loading boundary: project folders preload
`.rs`, `.toml` and `.md`; other suffixes can participate when already loaded by
configuration, such as a declared crate root. Discovery adds no suffix scan or
new macro loading capability. Included sources use the same repository,
readability, snapshot and file-count checks as modules.

In discovery, Cargo `test` and `bench` targets retain their test-only roles even
at custom paths. A file reused as both a target root and a module retains every
parent role: only entirely test-owned files are excluded. A production module
parent keeps the file, while a `cfg(test)` parent does not override test-only
target ownership. Actual include call locations join those roles; production
includes retain a shared file, while test-only nested includes remain excluded.
Pure static projects use the same native role queries. Explicit selection
preserves the existing declaration-level test filtering and parent query.
Production graph nodes remain limited to the resolved selection; unselected
repository and dependency declarations stay external.

The three dependency scopes and explicit existing network policy are unchanged.
Discovery adds no fetch/install step or network authorization and never executes
project build scripts or procedural macros. Tests for this change run offline.
See `analyzers/rust/src/tests/discovery.rs` for executable fixtures covering
workspace members, static projects, context, test roles, empty configuration,
snapshot consistency, malformed configuration and source boundaries.
