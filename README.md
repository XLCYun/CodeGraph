# CodeGraph

[English](README.md) | [简体中文](README.zh-CN.md)

CodeGraph provides seven standalone **Language Analyzers** that turn selected source files
into a common **Code Graph**: code objects and their relationships for one fixed source state.

Each analyzer uses its language's native semantic tooling. They share a [JSON protocol](docs/protocol.md),
so callers can use the same request, result schema and process handling across languages.

```text
Source directory + file selection → chosen Language Analyzer → Code Graph (JSON)
```

The analyzers run independently of Narrant or Tauri. The analyzed directory does not need to be
a Git repository. Graph differences and multi-language graph merging belong to consumers.

**Version:** This checkout targets **v0.1.1**, including the Go directive-text fix,
for **macOS ARM64** and **Windows x64**. Published builds appear on the
[release page](https://github.com/XLCYun/CodeGraph/releases/tag/v0.1.1) after both
native CI targets pass. Each manifest records the exact source commit;
package/engine version is `0.1.1` and protocol is `codegraph.analyzer/1`.
There is no package-manager installation.
Download the matching archive and manifest from the release and verify them against
[SHA256SUMS](https://github.com/XLCYun/CodeGraph/releases/download/v0.1.1/SHA256SUMS)
using the [distribution contract](docs/distribution.md) before invoking its direct entry.
For example, Python packages are available for
[macOS ARM64](https://github.com/XLCYun/CodeGraph/releases/download/v0.1.1/python-aarch64-apple-darwin-pack.tar.gz)
and [Windows x64](https://github.com/XLCYun/CodeGraph/releases/download/v0.1.1/python-x86_64-pc-windows-msvc-pack.tar.gz).

## Choose an analyzer

Choose the package for the source language, then invoke its entry with `analyze`.
Paths below are relative to the installed package; Windows entries end in `.exe`.

| Language | Package ID | Analysis engine | Public entry |
| --- | --- | --- | --- |
| [Rust](analyzers/rust/README.md) | `rust` | rust-analyzer | `bin/codegraph-rust` |
| [TypeScript / JavaScript](analyzers/typescript/README.md) | `typescript-javascript` | TypeScript compiler APIs | `bin/codegraph-typescript-javascript` |
| [Python](analyzers/python/README.md) | `python` | Ruff / ty | `bin/codegraph-python` |
| [Java](analyzers/java/README.md) | `java` | Eclipse JDT | `bin/codegraph-java` |
| [Go](analyzers/go/README.md) | `go` | `go/parser` / `go/types` | `bin/codegraph-go` |
| [C#](analyzers/csharp/README.md) | `csharp` | Roslyn | `bin/codegraph-csharp` |
| [C / C++](analyzers/clang/README.md) | `c-cpp` | Clang LibTooling | `bin/codegraph-c-cpp` |

Use each analyzer's CLI with the common JSON protocol to send requests and read results.
Follow the language links for supported project configurations and language-specific setup.

## Run an analyzer

Start with an **already-built, verified Analyzer Package**, installed outside the
source directory. From this checkout, the example wrapper analyzes a project
using its configured source discovery:

```sh
python3 scripts/examples/analyze-directory.py /absolute/verified/pack/bin/codegraph-typescript-javascript /absolute/project
```

On Windows, use the available Python 3 command and the `.exe` entry:

```powershell
python scripts/examples/analyze-directory.py C:/verified/pack/bin/codegraph-typescript-javascript.exe C:/project
```

The repository's example wrappers need Python **3.9+**. The Python analyzer itself
needs no Python interpreter. The wrapper sends UTF-8 input, closes stdin and
checks the final identity, graph fields and exit status; package verification
remains the caller's responsibility.

To invoke the analyzer directly, save this minimal request as UTF-8 `request.json`
and replace `repository_root` with an existing absolute source directory:

```json
{
  "protocol": "codegraph.analyzer/1",
  "type": "analyze",
  "id": "example-1",
  "repository_root": "/absolute/project",
  "options": {
    "include_tests": false,
    "rust_dependency_scope": "repository-only"
  }
}
```

In a **POSIX shell**:

```sh
"/absolute/verified/pack/bin/codegraph-typescript-javascript" analyze < request.json
```

This redirection supplies stdin EOF. The PowerShell example above uses the wrapper
to send UTF-8 input. In Windows JSON paths, `C:/project` avoids backslash escaping.

Every request field except `files` is required, including both `options` fields
for every language. Non-Rust analyzers ignore `rust_dependency_scope`. Source and
package directories must not overlap.

## Selection and results

| `files` in the request | Meaning |
| --- | --- |
| Omitted | Discover source using the language's selection policy. |
| Nonempty, such as `["src/main.ts"]` | Select exactly these files for complete graph nodes. Unselected source may provide semantic context or external symbols. |
| `[]` | Return an empty graph without project configuration or target SDK discovery. Source and installation boundary checks still apply. |

Selection paths are unique, repository-relative POSIX paths. `null`, duplicate
paths and unsafe paths are invalid. Discovery follows the language's project
membership, source inventory and include/exclude policies. See
[selection policies](docs/engine-selection.md).

The process reads one UTF-8 JSON object followed by EOF; a trailing newline is
optional. **EOF completes input.** Stdout contains one final JSON result followed
by a newline, then the process exits. Stderr carries diagnostics.

A successful result has `type: "completed"`, `outcome: "ok"` and a `graph` with:

| Graph field | Contents |
| --- | --- |
| `root_contains` | IDs of top-level code objects. |
| `nodes` | Code objects with identity, source, containment and semantic references. |
| `external_symbols` | Descriptors for referenced objects outside the complete node selection. |

References can target a graph node, an external symbol or an unresolved name.
Unresolved references are valid graph data. The [protocol](docs/protocol.md)
defines node categories, fields, limits and error results.

Before consuming a graph, require a complete successful result with matching
`protocol`, `engine_version`, `pack_id` and request `id`, **and exit status 0**.
A result alone, EOF alone or exit 0 without the matching result is insufficient.

The caller preserves the source state, drains stdout and stderr, and enforces
resource limits and deadlines. On cancellation, timeout or transport failure,
terminate the **whole process tree** and wait for the root and descendants to exit.
Keep the source snapshot and installed package unchanged until then. The example
wrapper does not implement this full production lifecycle; see the [integration contract](docs/protocol.md).

## Runtimes and target SDKs

An **Analyzer Package** includes the resources needed to run its analyzer. Its
**Analyzer Runtime** is separate from the **Project Target SDK** used to understand
the analyzed project's standard libraries, declarations or platform headers.

TypeScript/JavaScript, Java and C# packages bundle their Node, JDK and .NET execution
resources. Python uses a native executable with embedded typeshed declarations.
The [package layout](docs/pack-layout.md) lists each analyzer's runtime resources.

Project targets may still require existing SDKs. For example, a Java 17 project
needs a matching target JDK even when the analyzer runs on its bundled JDK 25;
C# needs matching reference assemblies, which a runtime-only .NET installation
does not provide. Target SDK discovery only reads existing installations. It
never installs or upgrades SDKs or runs project build scripts.

C/C++ can generate a source graph without `compile_commands.json` or installed
project headers. Missing semantic context leaves uncertain fragments opaque and
references unresolved. A usable compilation database adds verified bindings;
it does not change which selected source is represented. See the [C/C++ policy](docs/engine-selection.md#c--c-resolution).

## Build and verify

Use Node **24.18.0** and pnpm **11.13.1**. Prepare the fixed native toolchains and host build tools
using [distribution](docs/distribution.md) and the [toolchain manifest](resources/native-toolchains.json).
`pnpm install` only installs Node build dependencies. An all-seven build also needs locked Java JARs;
the Python 3 step below fetches and verifies them. Java build helpers also need Python 3.

From the repository root, with those prerequisites ready:

```sh
pnpm install --frozen-lockfile
python3 analyzers/java/tools/fetch-deps.py
pnpm build:packs --output /absolute/fresh/codegraph-packs
```

This builds all seven packages for the current supported host. Add `--id rust`
to build only Rust. The builder uses `--id`; provisioning and full verification
use `--language`. Existing artifact destinations are not overwritten, so use a
fresh output directory for each build.

Archives and manifests go under `<output>/<id>/0.1.1/<target>/`; verified extracted
packages go under `<output>/smoke-installed/<id>/0.1.1/<target>/`.

The root Cargo workspace builds only the TypeScript/Java thin launchers. Rust and Python use their
own manifests and Rust **1.98.0**. Run all Cargo commands through `scripts/cargo-agent.sh`; keep
incremental enabled and separate targets for divergent worktrees. The analyzer table above links
to language-specific build and test commands.

For local script checks, run `pnpm test` and `pnpm check:scripts`. Complete native
verification uses the command below after preparing the fixed SDK test fixtures
described in [distribution](docs/distribution.md):

```sh
node scripts/language-packs/verify-native-analyzers.mjs artifacts --build --language all
```

Local package builds and focused checks do not replace complete dual-platform CI.
See [test migration](docs/direct-test-migration.md) for the preserved test suites.

## Documentation

| Read this | For |
| --- | --- |
| [Public protocol](docs/protocol.md) | Requests, results, graph schema and caller lifecycle. |
| [Selection policies](docs/engine-selection.md) | Project discovery, exact selection and language constraints. |
| [Package layout](docs/pack-layout.md) | Entry points, bundled runtimes and required package files. |
| [Distribution](docs/distribution.md) | Tool preparation, package verification, CI and publication. |
| [Glossary](GLOSSARY.md) | Shared domain terms. |
| [Migration provenance](docs/migration.md) | Extraction history and retained responsibilities. |

---

[Apache-2.0](LICENSE). Extracted Narrant code retains its original licensing;
third-party components retain their licenses and notices. The extraction began
with [Narrant #205](https://github.com/XLCYun/narrant/issues/205).
