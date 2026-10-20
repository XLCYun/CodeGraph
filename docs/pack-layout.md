# Language package layout (0.1.0 development series)

Every trusted package contains `control/codegraph.json`. This descriptor is public
and separate from the private helper configuration:

```json
{"schema_version":1,"pack_id":"rust","pack_version":"0.1.0","engine_version":"0.1.0","protocol":"codegraph.analyzer/1"}
```

All fields are mandatory; unknown or duplicate fields fail. The compiled entry identity,
package version, engine version and protocol must exactly match the CLI. The file
must resolve inside the pack and is capped at 1 MiB. Distribution tooling owns
immutable archives, manifests, licensing and cryptographic verification; the caller
must verify these before selecting a trusted root. The descriptor alone is not an
integrity signature. This stage has no published archive.

Each package has exactly one public entry `bin/codegraph-<pack-id>[.exe]`.
It accepts `analyze`, `--help`, and `--version`; the language adapter is bound at
compile time. No generic executable, public language selector, runtime command
assembly, or `--pack-root` is needed. All runtime dependencies travel with the package.

`control/native.json` retains its private schema:

```json
{"schema_version":1,"helper_version":"0.1.0","platforms":[]}
```

An optional `target_sdk` object contains only `kind` (`java`, `go`, `dotnet`, `macos`,
`windows-msvc`) and selects fixed local SDK discovery. Each bundled platform contains
`id`, `kind`, `version`, `root` (pack-relative), and `metadata`. Metadata has the
existing language-specific allowlist. It cannot contain command or environment
injection. Fixed relative paths and symlink containment are checked by the factory.
No public request exposes this layout or the helper protocol.

| Pack ID | Private engine / runtime entry | Runtime / standard declarations |
| --- | --- | --- |
| `rust` | `bin/codegraph-rust-engine[.exe]` | Existing project Cargo/rustc; sysroot/rust-src discovered per scope |
| `typescript-javascript` | `bin/node[.exe]`, `runtime/helper/main.mjs` | `runtime/typescript/lib`, `runtime/@types/node`, `runtime/undici-types` |
| `python` | `bin/codegraph-python-engine[.exe]` | `python-embedded-stdlib` platform root `bin`, fixed ty_vendored revision |
| `java` | `runtime/jdk/bin/java[.exe]` | `runtime/helper/classes`, `runtime/helper/lib/*`, main `dev.codegraph.java.Main`; local target JDK discovery |
| `go` | `bin/codegraph-go-engine[.exe]` | Local Go SDK sources, highest stable patch in declared minor |
| `csharp` | `bin/dotnet[.exe]`, `runtime/helper/CodeGraph.CSharp.Engine.dll` | Helper `.deps.json`, `.runtimeconfig.json`, Roslyn DLLs; local target .NET reference packs |
| `c-cpp` | `bin/codegraph-clang-engine[.exe]` | Bundled Clang resources; approved local macOS or Windows/MSVC headers |

Preserve upstream versions and licenses in the helper manifests, lockfiles,
`licenses/`, Java dependency lock, C# `global.json`, and Clang `toolchain.json`.
Private engines use CodeGraph identities and `codegraph.engine/1`, not a second
CodeGraph consumer API. Sources under `analyzers/common` and
`analyzers/windows-process-probe` reference only CodeGraph's own
`src/child_process.rs`; no `src-tauri` source dependency remains.

Authoritative implementation links: [fixed factory](../src/adapters/mod.rs),
[platform validation](../src/native_analyzer/input.rs),
[public descriptor](../src/pack.rs), [Rust policies](../src/adapters/rust.rs).
Distribution tooling adds this descriptor and the version-matched fixed public entry without adding public fields to the
`deny_unknown_fields` private `native.json` schema.

Distribution manifests, artifact names, size limits, fixed URL templates and local
candidate paths are specified in [distribution.md](distribution.md).
