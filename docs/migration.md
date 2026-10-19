# Migration provenance
Source: `XLCYun/narrant` at `890ae4960aba2fc64946423093ff064c41d6ebf7`.
CodeGraph base: `def716c4e1244e440be374e35b56660e44405922`.

All tracked seven-language helper sources, tests, upstream pins and license files
were copied. No tool downloads, node_modules, Tauri crate, or build output was copied.
The LLVM license only loses one trailing blank line for whitespace validation;
its license text is unchanged.
The source Narrant checkout was read-only throughout this stage.

[Machine-readable comparison list](migration-files.json) gives each source/destination
path, SHA-256, unchanged status, and every difference-dependent test retained in Narrant.
The list is for review, not a release integrity manifest.
## Moved production responsibilities

| Source | CodeGraph | Necessary adaptation |
| --- | --- | --- |
| `native-helpers/` | Same paths | Common and Windows probe now reference CodeGraph-owned child-process code; common has a standalone test dependency/lockfile |
| `src-tauri/src/structure_review/code_graph/` | `src/code_graph/` | Remove diff module, API, CompareError and diff tests; retain generation/validation and add internal node iteration for serialization |
| `src-tauri/src/structure_review/native_analyzer/` | `src/native_analyzer/` | Crate paths changed; recursive JSON duplicate checker shared with CLI; no language projection changes |
| `src-tauri/src/structure_review/native_languages/` | `src/native_languages/` | Crate paths and internal PackId; private control read is bounded and contained; all factory/SDK/Rust policies retained |
| `src-tauri/src/child_process*` | `src/child_process*` | Unix snapshots include descendant ancestry and birth time, including observed processes that leave the group; private worker observes its domain while running |
| `src-tauri/src/language_support/model.rs` | `src/pack.rs` | Only small PackId/entry definitions copied; new public descriptor validation, no installation service |
| Cargo/cache launcher and TS smoke support | `scripts/` | Independent repository lock name; support used by retained helper tests, not a consumer API |

The new `src/cli/` and public `tests/cli.rs` implement the consumer protocol,
package selection, supervised worker, bounded stdio, result serialization and real
process lifecycle fixtures. `src/strict_json.rs` extracts the original recursive
duplicate-key decoder, preserving extension JSON semantics. All Rust private tests
stay in separate module `tests/` files.

The pure graph test `empty_graph_has_a_root_and_no_nodes` replaces its redundant
self-diff assertion with direct empty-node iteration. Test subprocess filter names
now use the standalone module paths. The shared configDir fixture was also copied.
## Difference-test ownership

No difference algorithm was copied or recreated. Pure graph assertions from the
following files migrated. Each complete test listed below depends on `.diff()` and
remains in Narrant at the source revision. Task 3 must adapt its analyzer setup to
consume the public CLI and retain its assertions. This is a location change for
acceptance, not permission to discard these tests.
### `native_analyzer/tests/python_graph.rs`

- `function_body_change_belongs_to_callable`
- `class_method_body_and_local_class_do_not_change_parent_content`
- `conditional_declarations_preserve_execution_order_without_copying_child_content`
- `fields_deduplicate_receiver_bindings_without_stealing_method_content`
- `normalization_and_field_order_distinguish_semantic_content`
- `shared_initializers_and_repeated_fields_are_not_duplicated`

### `native_analyzer/tests/python_native.rs`

- `native_overload_docstrings_participate_in_content_comparison`

### `native_analyzer/tests/rust_graph.rs`

- `rust_helper_compares_prepared_repositories`
- `rust_helper_keeps_closure_references_without_closure_nodes`
- `rust_helper_resolves_graph_local_field_types`
- `rust_helper_omits_non_field_type_uses_and_data_accesses`
- `rust_helper_preserves_crate_roots`
- `rust_helper_excludes_test_code_before_graph_analysis`
- `rust_helper_all_dependency_profiles_use_real_project_platform`
- `rust_helper_syntax_diff_nested_items_in_type_expressions`
- `rust_helper_syntax_diff_preserves_literal_contents`
- `rust_helper_syntax_diff_preserves_usage_but_ignores_item_order`
- `rust_helper_syntax_diff_top_level_macro_arguments`
- `rust_helper_syntax_diff_macro_punctuation_jointness`
- `rust_helper_syntax_diff_excludes_nested_tests_under_glob_imports`
- `rust_helper_syntax_diff_excludes_nested_test_edits`
- `rust_helper_whole_graph_project_form_matrix`

### `native_analyzer/tests/rust_syntax.rs`

- `preserves_original_source_without_changing_structural_comparison`
- `nested_body_edits_belong_only_to_their_owner`
- `closures_are_code_of_their_enclosing_function_or_value`
- `named_items_inside_closures_keep_their_own_content`
- `adding_fields_does_not_modify_the_type_own_content`
- `nested_items_in_type_expressions_have_separate_content`
- `normalization_preserves_literal_contents`
- `preserves_child_expression_usage_but_ignores_item_order`
- `top_level_macro_invocations_have_content`
- `macro_punctuation_jointness_is_significant`
- `unicode_and_crlf_keep_precise_source_and_stable_identity`

### `native_languages/rust/tests/production.rs`

- `production_profiles_keep_graph_policy_and_readonly_project`

### `native_languages/tests/typescript_graph.rs`

- `graph_test_membership_lazy_context_and_stable_external_symbols`
- `graph_diff_format_child_parent_and_inline_callback`
- `graph_dynamic_unknown_names_ignore_format_but_preserve_literal_values`
- `typescript_jsx_leading_and_space_only_text_changes_graph_content`

The complete `code_graph/tests/diff.rs` suite and Narrant application/Review
comparison tests also remain in Narrant unchanged. Production Rust `diff.rs`,
`CompareError`, and public difference types are explicitly excluded.

## Verification boundary

This extraction runs the small standalone engine, graph/Host/factory/process tests,
public CLI subprocess fixtures, cache script tests and common checks. Real helper
semantics still require prepared packs. Existing environment names such as
`NARRANT_RUST_HELPER`, `NARRANT_PYTHON_HELPER`, and `NARRANT_TYPESCRIPT_PACK` remain
private test harness variables; they are not accepted in public JSON. The common
`tests/initial.rs` is compiled by Rust/Python helper integration targets, not the
common library unit target.

Windows cross-checking can catch API/type errors locally but does not replace
Windows runtime lifecycle tests. No release, GitHub operation or published artifact
is part of this stage. Downstream contracts are [protocol.md](protocol.md) and
[pack-layout.md](pack-layout.md).

## Task 2: independent distribution and acceptance

CodeGraph now owns `resources/language-support-packs.json`, unchanged
`resources/native-toolchains.json`, all native build/provision/license/archive
scripts, independent Node dependency lock, full PR CI and immutable-release
preparation. The old helper pins and semantics are unchanged; the three original
`1.1.0` package identities become a separate CodeGraph `0.1.0` series. Every pack
contains the same versioned public CLI. See [distribution.md](distribution.md) for
exact artifact URLs, manifest fields, local candidate roots and release conditions.
The source checkout remains read-only.

`resources/semantic-acceptance.json` enumerates every retained real-pack ignored
engine test: 69 on macOS, plus the Windows junction boundary case on Windows.
The verifier requires each named test to report `ok`; zero-test Cargo success cannot
pass. The internal ignored `child_process::test_support::fixture::process_fixture`
is invoked only by its owning subprocess tests and never by a blanket ignored run.
Missing requested SDK fixtures fail before suites can skip them. Independent groups
run serially, retain original failures and aggregate errors without a false success.

| Original verifier command/group | CodeGraph destination / preserved acceptance | Narrant Task 3 ownership |
| --- | --- | --- |
| Go `go test ./...` | Same command, fixed build + target SDKs, offline settings | Review/Diff only |
| Java `unittest discover native-helpers/java/tests` | Same complete suite; JDK 8/17/25 declarations, JDT source/protocol/lifecycle | Review/Diff only |
| C# `unittest discover native-helpers/csharp/tests` | Same complete suite; real net8/net9 nonzero patch/net10 reference roots | Review/Diff only |
| C/C++ `unittest discover native-helpers/clang/tests` | Same complete suite; LLVM resources and exact platform SDKs | Review/Diff only |
| TS nine Node test files | Same files, `--test-concurrency=1`, configuration, readonly host, projection, protocol, output and lifecycle | None of these tests deferred |
| `native_languages::typescript_graph:: --ignored` | 22 retained test names, same Rust assertions | Four listed diff-dependent tests retained above |
| Rust helper `+1.98.0 test --lib --test initial_protocol --test lifecycle --include-ignored` | Same command, same runner stripping only Cargo build-tool selection, historical fixture matrix | None of these helper tests deferred |
| `native_analyzer::tests::rust_graph:: --ignored` | 11 retained names including source/provenance, three dependency contexts, read-only loading and lifecycle | Fifteen diff-dependent tests listed above |
| `native_analyzer::tests::rust_syntax:: --ignored` | Four retained source/closure/cfg tests | Eleven diff-dependent tests listed above |
| `native_languages::rust::tests:: --ignored` | Production cancellation and process-probe ownership | One production profile diff test listed above |
| Rust/TS real installed language_support tests | New real archive verifier and real public CLI graph validation replace CodeGraph's app dependency | Application installer/service and cache assertions remain in Narrant |
| Rust/TS Review-to-DTO tests | Public graph acceptance covers producer boundary | Complete Review lifecycle and DTO assertions remain in Narrant |
| Python helper `+1.98.0 test --lib --test initial_protocol --test lifecycle` | Same full helper/protocol/lifecycle suite | None of these helper tests deferred |
| `native_analyzer::tests::python_graph:: --ignored` | Ten retained tests | Six diff tests listed above |
| `native_analyzer::tests::python_native:: --ignored` | 21 retained tests on macOS, 22 on Windows | One overload/docstring diff test listed above |
| `language_support::python_tests::` | Real Python archive integrity + public CLI acceptance replace CodeGraph application setup | Installation/caching/service tests remain in Narrant |
| `application::tests::python::` | Public CLI graph acceptance | Review/DTO consumer semantics remain in Narrant |
| `child_process::`, `native_analyzer::`, `native_languages::` non-ignored | Full `--bin codegraph` suite (no public lib), serial tests; all retained assertions run | Application-specific wrappers only |
| `structure_review::application::tests::native::` | Public CLI boundary/lifecycle integration tests | Review application suite remains in Narrant |
| `real_native_packs_install_and_analyze_through_production_factory` | All seven archives verified/unpacked; actual package CLI and standalone CLI exercised through fixed factory | App installation service assertions remain in Narrant |
| `native_helper_graphs` graph/diff integration | Public CLI local-call/source/read-only/test-switch graph acceptance for seven packs | Original graph/diff comparisons remain in Narrant; no diff copied |
| Windows process probe/lifecycle scripts | Same shared production owner crate, early native probe, locked wrapper | Narrant app manifest/Common Controls behavior remains in its app tests |
| Old pack script tests | Archive paths/permissions, licenses, SDK fixtures, pinned toolchains, runner quoting, serial failure aggregation, XZ cleanup, redist selection retained; new CLI/evidence/release failure tests | App-only workflow and application signing tests not copied |

The original publisher and `public-draft-assets.mjs` were not reused: they depend on
Narrant's app release layout. A small independent publisher uses official `gh`
asset API operations, verifies exact CI/source identity and all asset bytes, and
has dedicated immutable-version and failed-upload tests. It never accesses Narrant
releases. Original helper README historical text is provenance, not CodeGraph
publication evidence; this repository's distribution document is authoritative.

Task 2's real CLI acceptance also exposed a macOS cancellation race in the shared
process owner: a group containing only an unreaped zombie returns `EPERM` to group
signals. A deterministic `waitid(WNOWAIT)` regression failed before the fix. The
owner now accepts that signal race only after both direct Child wait and the full
group/observed-descendant exit barrier succeed. A permission-denied group probe
means still alive, never disappeared; other errors and live-member timeouts still
fail. No protocol or timeout changed. Separate owner tests and repeated real CLI
cancellation verify this change; Narrant should carry the reviewed owner fix when
adapting its consumer in Task 3.
