# Public integration tests

This `publish = false` crate is compiled only under `cfg(test)`. It is not a Rust
SDK and never ships in a language package. Its caller sends one public JSON to the
real installed `bin/codegraph-<id>[.exe] analyze`, closes stdin, drains stdio,
checks final compiled identity and exit status, and owns process-tree termination.
The shared caller utility lives in `analyzers/windows-process-probe`; neither crate
is a production analyzer or supervisor.

The graph view only deserializes and indexes returned data. It does not regenerate,
repair, deduplicate, alter ID/content/edges or expand selection. Original semantic
assertions and frozen TypeScript/Python/PR31 fixtures remain identifiable in
`resources/semantic-acceptance.json` and `resources/direct-test-migration.json`.

Set `CODEGRAPH_NATIVE_PACKS` to an exact-source artifact output containing
`smoke-installed/<id>/0.1.0/<target>`, and expose already-installed target SDKs through
standard discovery variables. Rust additionally uses
`CODEGRAPH_PROJECT_RUST_TOOLCHAIN=stable`; the actual Cargo/rustc probes preserve
per-project override tests. The Python deadline/panic fixture waits for its actual
configuration-read milestone and PID, then terminates through the same test-only
ProcessTree owner and drains/joins both real pipes. It uses no shell startup recorder.

Error views preserve the actual public error code/message and separately captured
bounded stderr. Existing detailed diagnostic assertions read those actual diagnostics;
they never synthesize private error text or change the public response.

```sh
scripts/cargo-agent.sh test --manifest-path analyzers/integration-tests/Cargo.toml --locked --lib tests::view::
scripts/cargo-agent.sh test --manifest-path analyzers/integration-tests/Cargo.toml --locked --lib tests:: -- --ignored --test-threads=1
```

Use `verification-commands.mjs` for per-language filters and mandatory named-success
checks. Fixture application `#[test]` text is passed to the analyzer as source and
never compiled as application tests. Real Windows spawn/Job/DLL/pipe evidence still
requires the automatic Windows CI job; Unix process groups cannot certify it.
