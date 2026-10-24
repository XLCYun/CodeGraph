# Go native analyzer

The fixed `bin/codegraph-go[.exe] analyze` entry directly implements
`codegraph.analyzer/1`. It reads one UTF-8 JSON object through EOF, writes one
final result with `protocol`, `engine_version: "0.1.0"`, and `pack_id: "go"`, then
exits. EOF completes input normally. The caller owns deadlines and process-tree
termination; no ready/cancel messages, worker or stdin listener exists.

The compiler is pinned independently of project targets. Language capability
comes from `runtime.Version()` and `go/version`. Analysis never executes `go`, a
project script, a dependency or a shell. See [the public protocol](../../docs/protocol.md).

```sh
cd analyzers/go
GOTOOLCHAIN=local GOPROXY=off "$GO_COMPILER_ROOT/bin/go" test ./...
GOTOOLCHAIN=local GOPROXY=off "$GO_COMPILER_ROOT/bin/go" build -o bin/codegraph-go .
CODEGRAPH_GO_TARGET_SDK=/absolute/go1.23.8 \
  python3 -m unittest discover -s tests -v
```

On Windows the output is `bin/codegraph-go.exe`. The native executable needs no
Go runtime. It discovers target standard-library sources from
`CODEGRAPH_GO_TARGET_SDK`, `CODEGRAPH_GO_SDK`, `GOROOT`, conventional install
locations, `~/sdk/go*`, Homebrew and resolved PATH entries. Discovery only reads
files. It freezes the highest stable patch for each installed minor; project
selection then applies the policies below. SDK roots cannot overlap the source
repository. Internal typed component fixtures retain explicit targets and source
evidence; those fields are never public request inputs or result fields.

Version selection follows the repository's static declarations:

- Without `go.work`, each selected independent module receives its own context and SDK selection. Selecting several modules does not make them an implicit workspace.
- A workspace uses its `go` declaration and its non-`default` `toolchain` suggestion. The effective minimum is the higher of those versions; module `toolchain` suggestions do not override the workspace. The workspace's `go` version must be at least every used module's `go` version.
- A standalone module applies the same rule to its own `go.mod`. `toolchain default` adds no version suggestion: this profile uses the declared `go` version and does not read or emulate the user's `GOTOOLCHAIN` settings.
- For each OS/architecture, selection uses the highest approved stable patch in the effective minor, provided it meets the exact minimum patch. It never borrows a newer minor when that target is absent. Duplicate SDK entries do not create duplicate configurations.
- Missing minor/patch targets return `target_unavailable`. A required language newer than the helper's actual compiler returns `unsupported_configuration`. SDK patches within the compiler's supported minor share its language capability.
- Each repository package is checked with its own module's `go` language version, including explicit local replacement modules. Standard-library packages use the selected SDK's version. Thus a newer workspace SDK does not upgrade older modules' language rules.

File selection uses the selected SDK's release tags, including correct handling of guards for future releases. The helper also statically reads that SDK's default experiment baseline, so the compiler used to build the helper cannot inject its own experiment tags into an older target. This reader accepts only boolean/string declarations, comparisons, and target switches preceding the SDK's baseline literal. Unknown shapes or customized default experiments fail explicitly. It does not execute SDK code. cgo and user-supplied custom build tags remain disabled.

The loader reads the repository's `go.mod`/`go.work`, workspace modules, and unversioned local directory `replace` targets inside the repository, plus approved SDK standard-library sources. It never consults the module cache, GOPATH, network, `go list`, or compiler export data. Third-party dependency source remains excluded even when present on disk. Missing third-party imports and APIs unavailable in the selected SDK can leave relationships unresolved while complete source declarations remain in the graph. A used local replacement must provide its own readable `go.mod` so its package language version is known.

Source that violates its package's effective Go language version returns `object_extraction_failed` with no graph, including violations in unselected files and imported repository package bodies. This uses the pinned compiler's public `types.Error.Msg` version diagnostics, without accessing private error codes. The selected Go 1.23 SDK also retains its disabled-by-default generic-alias experiment; newer SDKs retain their native compatibility rules. A supported per-file release guard can still raise the file's effective language version.

Without explicit platform constraints, the helper analyzes its native GOOS/GOARCH baseline. Otherwise it inspects the admitted package/import closure, including unselected platform files, and evaluates supported Boolean `//go:build` / legacy `+build` conditions against the catalog. Every selected non-test file must be active in an analyzed target. External test packages, vendor, embed, custom tags, implicit nested modules, and ambiguous configuration remain outside this static profile. Workspace `replace` wins over module `replace`; conflicting module replacements without an override fail.

The tests use actual SDK `VERSION` files; they never relabel one SDK source tree as another version. General graph tests use the test compiler's real SDK. Cross-minor regressions require a real Go 1.23 installation and a second installation at Go 1.24 or later that the helper compiler supports. Provide their roots using the platform's path-list separator:

```sh
CODEGRAPH_GO_TEST_SDKS="/path/to/go1.23.8:/path/to/go1.27.1" \
  GOTOOLCHAIN=local GOPROXY=off "$GO_COMPILER_ROOT/bin/go" test -v ./...
```

The tests also inspect `runtime.GOROOT()` and `/usr/local/go` when present. If real cross-minor targets are absent, those cases explicitly skip; that is not evidence of cross-version support. They cover SDK patch minimums, missing/future targets, `toolchain`/workspace precedence, per-package language rules, independent modules, old-SDK API absence, release guards, and default experiment isolation. The previous Go 1.23.8 validation record remains [historical implementation evidence](../../docs/research/go-native-implementation.md); it does not establish acceptance for an untested SDK or host platform.

## 目录发现

公共 `files` 省略时，由本语言配置加载器确定完整选集；`[]` 是明确空选集，
`null` 无效。显式数组保持精确选择。具体成员、排除规则、测试角色与限制见
[引擎选集政策](../../docs/engine-selection.md)。新增目录发现测试由现有原生验收命令运行，
来源、图不变量在本进程输出边界检查；`tests/test_public.py` 验证真实直接入口与调用方终止。

Internal component failures retain their detailed language codes; public failures use the stable codes in `docs/protocol.md`.
