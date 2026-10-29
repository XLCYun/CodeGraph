# Language package layout (0.1.2)

Each verified package exposes exactly one public entry
`bin/codegraph-<pack-id>[.exe] analyze`. There is no required umbrella program,
private analysis hop, worker, runtime command assembly or public Rust SDK.
The program itself reads the public JSON, discovers target SDKs and emits its
final Code Graph. See [protocol.md](protocol.md).

Every package contains the exact public descriptor `control/codegraph.json`:

```json
{"schema_version":1,"pack_id":"rust","pack_version":"0.1.2","engine_version":"0.1.2","protocol":"codegraph.analyzer/1"}
```

The descriptor is bounded to 1 MiB and has exactly these fields. The caller verifies
the immutable manifest, hashes, paths and archive before trusting the installation;
the descriptor alone is not an integrity signature. Final compiled identity must
match the installed package. Programs do not accept catalog, SDK, runtime or command
paths in their JSON request.

| Pack | Entry implementation | Required runtime and analysis resources |
| --- | --- | --- |
| `rust` | Native Rust, in-process rust-analyzer | No analysis worker; installed project Cargo/rustc and rust-src according to scope |
| `typescript-javascript` | Thin native launcher and JS program | `bin/node[.exe]`, every production module in `runtime/analyzer/`, fixed `runtime/typescript`, `runtime/@types/node`, `runtime/undici-types` |
| `python` | Native Rust, in-process Ruff/ty | Fixed embedded typeshed; no Python interpreter |
| `java` | Thin native launcher and Java program | `runtime/jdk/bin/java[.exe]`, production `runtime/analyzer/classes`, all locked `runtime/analyzer/lib/*.jar` |
| `go` | Native Go | No execution runtime; installed target SDK sources discovered in-process |
| `csharp` | SDK-generated self-contained apphost | Complete publish directory in `bin/`: managed entry, deps/runtimeconfig, Roslyn, BCL, CLR, hostfxr/hostpolicy, native sidecars and resource directories |
| `c-cpp` | Native C++ with Clang LibTooling | `runtime/platform/clang-resource/include/stddef.h` and all fixed Clang resource contents; macOS `runtime/lib/libclang-cpp.dylib`; actual required Windows DLLs adjacent to EXE in `bin/` |

TS/Java launchers only resolve package-relative runtime/resources, set fixed argv
and necessary environment, inherit stdio and propagate exit. macOS exec replaces
the launcher; Windows spawn/wait preserves caller ownership of the whole subtree.
C# never executes a caller-built dotnet command or falls back to an installed
execution runtime. Its managed assembly is AnyCPU while the apphost and CLR match
the package RID (`osx-arm64` or `win-x64`). The fixed runtime is 10.0.0 and roll-forward
is disabled. Both RID NuGet runtime archives have official source and SHA512 pins
in [native-toolchains.json](../resources/native-toolchains.json).

C/C++ uses executable-relative rpaths `@executable_path/runtime/lib` and
`@executable_path/../runtime/lib` on macOS. Windows receives the actual CMake runtime
DLL closure and approved fixed VC143 CRT; static LLVM does not imply invented DLLs.
The resource directory contains the contents of LLVM `lib/clang/21`, without an
extra nested `21` directory. System target headers remain installed SDK inputs.

`control/native.json` remains producer package metadata (`schema_version`,
`helper_version`, `platforms`, optional `target_sdk.kind`). It is not a private
message interface and the programs do not require a caller to interpret it.
Java/Go/C# platforms are empty and target declarations are not copied into
`runtime/platform`. Python describes its embedded declaration revision; C/C++
describes fixed builtin resources. No request can override this metadata.

All upstream notices and locked-dependency licenses remain in the archive.
C# includes both SDK and runtime pack notice variants. Java JDK legal files are
retained. `ComponentDriver`, `codegraph-clang-components`, C# Components assemblies,
Go test binaries, `analyzers/integration-tests` and process-probe utilities never ship.
Old `*-engine` binaries and `runtime/helper` are rejected by the package verifier.

[distribution.md](distribution.md) defines immutable manifests, fixed asset names,
size limits, source receipts and local candidate handoff.
