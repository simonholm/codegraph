# codegraph

A small experimental Rust CLI that extracts a repository-local function-call graph
as JSON and navigates saved graphs. Rust syntax and call resolution come from
rust-analyzer over LSP. There is no Rust parser in this project, database,
visualization, or AI integration.

## Run

Requirements: Cargo and a Rust toolchain compatible with the target repository,
a working `rust-analyzer` executable, and available target-repository dependencies.
Install rust-analyzer and standard-library sources with rustup if needed:

```sh
rustup component add rust-analyzer rust-src
cargo build --locked --target-dir target
./target/debug/codegraph tests/fixtures/simple-rust
```

With `target/debug` on PATH, the equivalent command is:

```sh
codegraph tests/fixtures/simple-rust
```

For a standalone server outside PATH:

```sh
RUST_ANALYZER=/path/to/rust-analyzer ./target/debug/codegraph tests/fixtures/simple-rust
```

`RUST_ANALYZER` is an executable path, not a shell command. JSON goes to stdout;
warnings and errors go to stderr. Failed extraction exits nonzero without emitting
a graph. Workspace warnings can still accompany a successful, incomplete graph.

## Navigate a saved graph

```sh
codegraph /path/to/rust-repository > graph.json
codegraph callers rust::discover_projects --graph graph.json
codegraph callees scan::report --graph graph.json
codegraph trace main rust::discover_projects --graph graph.json
```

The exact syntax is:

```text
codegraph <rust-repository-path>
codegraph callers <function> --graph <graph.json>
codegraph callees <function> --graph <graph.json>
codegraph trace <from> <to> --graph <graph.json>
```

Queries read the saved JSON and require neither rust-analyzer nor the original
repository. `--graph` is required after the function selectors. There is no
implicit cache or default graph file. Regenerate the graph after source changes.

- `callers` lists direct incoming relationships; `callees` lists direct outgoing
  relationships. Results are unique and sorted, with file/line/column IDs.
- `trace` uses breadth-first search to return a shortest directed path. Neighbor
  IDs break ties deterministically. Cycles are handled; tracing a function to
  itself returns that function alone. A path describes recorded call relationships,
  not runtime ordering or unconditional execution.
- Selectors are case-sensitive exact symbol paths (`impl Recall::search`,
  `Adapter::search`), bare names (`discover_projects`), file-based
  qualified aliases (`rust::discover_projects`), or exact node IDs
  (`src/rust/mod.rs:154:8`). `src/rust/mod.rs` becomes `rust`, and
  `src/clean/shared.rs` becomes `clean::shared`; crate-root `main.rs`/`lib.rs`
  have no prefix. These aliases are not semantic Rust item paths: inline modules,
  impl types, `#[path]` modules, and custom layouts can differ.
- Ambiguous selectors list every matching candidate with its symbol path (when
  available) and exact ID, and exit nonzero. Use a symbol path, qualified alias,
  or the listed exact ID to disambiguate. Even symbol paths and qualified aliases
  can collide. Missing functions and missing paths exit nonzero with a clear
  message. Empty caller/callee lists are reported explicitly and exit successfully.
- Nodes optionally include `symbol_path`; original IDs, names, and edges are
  preserved. Older saved graphs remain queryable with their original display
  aliases. Regenerate them to obtain richer identities. Loading checks unique
  node IDs and rejects edges with missing endpoints.

On the validated disk-maint graph, `callers rust::discover_projects` finds seven
callers, including cleanup planning, build-artifact discovery, reporting, and
tests. `callees scan::report` finds thirteen direct callees. The trace example
returns `main -> run -> rust::report -> rust::discover_projects`. These queries
expose shared dependencies and dispatch routes without manually joining JSON IDs.

## Function identity investigation

On rust-analyzer 0.3.3065-standalone, a live Recall probe found that hierarchical
`textDocument/documentSymbol` responses place functions under named modules,
traits, and impl blocks. Impl labels include the target type and, for trait
implementations, the trait: `impl Recall`, `impl Adapter for StaticAdapter`.
Function symbols also include a signature in `detail`, symbol kind, full range,
and selection range. rust-analyzer produces this hierarchy from source structure;
it is useful context, not a resolved, globally unique Rust type/item identity.
See its [document-symbol structure implementation](https://github.com/rust-lang/rust-analyzer/blob/master/crates/ide/src/file_structure.rs).

`prepareCallHierarchy` returns `CallHierarchyItem`s containing the function name,
kind, signature in `detail`, URI, range, and selection range. The four Recall
`search` items have indistinguishable signatures within the declaration/inherent
pair and within the implementation pair. They have no container/type/trait field;
rust-analyzer does not populate their optional opaque `data` field. Its
[call-item conversion](https://github.com/rust-lang/rust-analyzer/blob/master/crates/rust-analyzer/src/lsp/to_proto.rs)
confirms this. Standard LSP allows optional details, but does not prescribe a
structured enclosing impl identity for call items.

Hover shows type/trait context, but as formatted text rather than structured
identity data. The tested server rejects `textDocument/moniker` as an unknown
request. `experimental/parentModule` returns module-navigation locations (the
core Cargo manifest for these crate-root functions), not an enclosing impl.
The documented rust-analyzer extensions provide workspace-symbol scope/kind
filters and debug syntax/HIR/item-tree dumps, but no structured enclosing
type-and-trait identity suitable for this small change. Parsing hover or debug
text would require an additional, fragile interpretation layer. See the
[extension documentation](https://rust-analyzer.github.io/book/contributing/lsp-extensions.html).

Extraction now saves an optional `symbol_path`: names from the server's named
item hierarchy, joined with `::`. Impl labels are retained verbatim, including
generics and trait context; codegraph does not parse or rewrite them into
`Type::method` or `<Type as Trait>::method`. Only an exact URI and selection-range
match attaches a document-symbol path to a semantically prepared call item.
Outgoing-call targets preserve any identity already discovered. Targets without
a matching document symbol retain the original file-based display alias.
No source/path/line-number heuristics infer containers. Paths are local to the
document hierarchy, without inferred crate names or out-of-file module parents;
duplicates remain ambiguous, and exact IDs always work. Quote paths containing
spaces or shell metacharacters.
A top-level symbol path identical to a bare name still matches every function
with that name; it does not silently override bare-name ambiguity.

The regenerated Recall graph at `/tmp/recall-graph-with-identities.json` contains
the same 694 nodes and 1,225 edges, with unchanged IDs/names and edge sets.
682 nodes have symbol paths; 12 call targets lack matching document symbols and
retain file aliases. The four functions are displayed and selectable as:

| Exact ID | Symbol path / selector |
| --- | --- |
| `crates/core/src/lib.rs:2173:8` | `Adapter::search` |
| `crates/core/src/lib.rs:2272:12` | `impl Recall::search` |
| `crates/core/src/lib.rs:2781:12` | `tests::impl Adapter for StaticAdapter::search` |
| `crates/core/src/lib.rs:2813:12` | `tests::impl Adapter for LexicalAdapter::search` |

```sh
codegraph callees 'impl Recall::search' --graph /tmp/recall-graph-with-identities.json
```

This lists `Adapter::search`, `apply_project_ownership_boost_to_results`, and
`sort_search_results`. Its callers include the CLI dispatcher `run`. The old
`crates::core::src::lib::search` selector remains ambiguous and now lists these
richer identities. Unambiguous old selectors continue to work unless a newly
available symbol path collides with them; all matching selector forms are checked
together rather than silently choosing one. Recall was not modified.

## Test exclusion investigation

`--exclude-tests` is not implemented. The existing graph includes test functions
and helpers; query rankings and caller lists can therefore include tests.

Before deciding, we inspected document symbols and rust-analyzer's
`experimental/runnables` and `experimental/discoverTest` responses on a small
fixture containing both a mixed production/test module and a `#[cfg(test)]`
module with an ordinary helper. Document symbols supplied names, ranges, kinds,
and deprecated tags, but no test-only attribute. Runnables/test discovery identified
tests and containing modules, not a reliable production/test classification for
every ordinary helper. The mixed module also appeared as a test-module runnable;
excluding its entire range would incorrectly remove production code.

Reliable general filtering would need more Cargo target/cfg/attribute analysis
and helper classification. Path names, module names, or reachability from tests
alone are insufficient, so this iteration does not add a heuristic filter. See
the [rust-analyzer test API](https://rust-analyzer.github.io/book/contributing/lsp-extensions.html#test-explorer).

## How it works

1. Walk `.rs` files below the supplied root, which must contain `Cargo.toml`.
   Skip hidden directories, symlinks, and conventional generated/cache directories.
2. Spawn `rust-analyzer` with no arguments, with the repository as its working
   directory. Communicate using JSON-RPC 2.0 over piped stdin/stdout, framed with
   `Content-Length` headers (byte lengths).
3. Send `initialize` and `initialized`, verify document-symbol and call-hierarchy
   capabilities, and wait for rust-analyzer's `experimental/serverStatus`
   notification with `quiescent: true`. This startup barrier is the one server
   extension; graph extraction uses standard LSP requests.
4. Request `textDocument/documentSymbol` for each file. Recursively select Function
   and Method symbols and retain named item-container hierarchy, then request
   `textDocument/prepareCallHierarchy` at each symbol's selection position.
   Preserve the returned call-hierarchy items and attach exactly matched symbol paths.
5. Request `callHierarchy/outgoingCalls` for each discovered function, using its
   returned item. Retain function targets under the repository root.
6. Deduplicate and sort nodes/edges by ID, send `shutdown`/`exit`, and reap the
   server process. Startup and each request have a 60-second timeout.

Nodes have `id`, `name`, and optionally `symbol_path`; edges have `from` and `to`
node IDs. IDs use relative
file path and one-based line/column of the function name. Columns follow LSP UTF-16
positions. IDs distinguish repeated names but change when source positions move.
Repeated calls from one function to another produce one edge.

The initial live capability probe confirmed that rust-analyzer advertises
`documentSymbolProvider` and `callHierarchyProvider`, and returns the fixture's
calls directly. See the [LSP call-hierarchy specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#callHierarchy_outgoingCalls)
and [rust-analyzer's request handlers](https://github.com/rust-lang/rust-analyzer/blob/master/crates/rust-analyzer/src/handlers/request.rs).

## Fixture output

`tests/fixtures/simple-rust` is an independent Cargo project with four functions.
The CLI produces:

```json
{
  "nodes": [
    { "id": "src/main.rs:10:4", "name": "save", "symbol_path": "save" },
    { "id": "src/main.rs:12:4", "name": "helper", "symbol_path": "helper" },
    { "id": "src/main.rs:1:4", "name": "main", "symbol_path": "main" },
    { "id": "src/main.rs:6:4", "name": "parse", "symbol_path": "parse" }
  ],
  "edges": [
    { "from": "src/main.rs:1:4", "to": "src/main.rs:10:4" },
    { "from": "src/main.rs:1:4", "to": "src/main.rs:6:4" },
    { "from": "src/main.rs:6:4", "to": "src/main.rs:12:4" }
  ]
}
```

## Checks

```sh
cargo check --locked --target-dir target
cargo check --locked --manifest-path tests/fixtures/simple-rust/Cargo.toml --target-dir target/fixture
cargo check --locked --manifest-path tests/fixtures/identities-rust/Cargo.toml --target-dir target/identity-fixture
cargo test --locked --target-dir target
cargo fmt --check
cargo clippy --locked --target-dir target --all-targets -- -D warnings
```

Set `RUST_ANALYZER` for tests as well if the executable is outside PATH. The
integration test requires a real server and is never silently skipped: it checks
exactly four unique function nodes and the three known edges, including no extra
edges or dangling endpoints. Other tests check CLI usage and LSP byte framing,
including Unicode and truncated messages. Deterministic saved-graph CLI tests
cover callers, callees, shortest directed paths, cycles, ambiguity, exact-ID
selection, missing functions, missing paths, and empty neighbors. They run with
an unavailable analyzer to verify that navigation is independent of extraction.
Additional checks cover qualified-alias collisions and malformed graph endpoints.
The identity fixture checks eight same-file `search` functions: trait declarations,
an inherent method, different trait impls on the same type, a generic impl, and
free functions in different scopes. Exact identities, IDs, and edges are asserted.
Saved-graph identity tests exercise richer selectors, ambiguity messages, repeated
symbol paths across files, old selectors, callers/callees, and tracing without LSP.

Validated locally with Rust 1.98.1 and rust-analyzer 0.3.3065-standalone. Installing
`rust-src` for the active toolchain changed the real disk-maint graph from
185 nodes / 204 edges to 185 nodes / 262 edges: 58 added edges, none removed.
Standard-library type information materially improved repository-local call
resolution, so install `rust-src` for the toolchain selected by the target repo.
The fixture alone did not reveal that difference. Seventeen sampled emitted
relationships were checked against disk-maint source and matched; this was not
an exhaustive completeness check.

## Existing repositories and limits

```sh
./target/debug/codegraph /absolute/path/to/existing-rust-repository > graph.json
```

Use a Cargo package/workspace root and ensure its toolchain and dependencies are
available to rust-analyzer. No codegraph configuration or repository modification
is required. rust-analyzer can invoke Cargo metadata, which can fetch dependencies
or create/update Cargo lockfiles. Analyze trusted repositories.

- Only functions (including methods) and repository-local call edges are emitted.
  External dependency/standard-library targets are omitted; call-site ranges and
  runtime call counts are not recorded.
- Build-script execution, procedural-macro expansion, and check-on-save are
  disabled. Generated code and calls requiring those features can be missing.
- This is rust-analyzer's static call hierarchy, not a complete runtime graph.
  Indirect calls, dynamic dispatch, closures, macros, conditional compilation,
  and unresolved types can limit results. Verified omissions in disk-maint include
  calls inside macros, function values supplied to `map`/`unwrap_or_else`, and
  concrete targets of indirect cleanup callbacks. Closures can attribute calls to
  their enclosing function. Navigation uses those existing relationships as-is;
  a missing path is not proof that execution cannot reach a function, and an
  isolated function is not proof of dead code.
- File discovery is a directory walk, not a Cargo source inventory. Unlinked files
  may yield no semantic functions; unresolved prepared symbols are skipped with
  a warning. Cargo members/sources outside the supplied root are not scanned.
- Large workspaces can exceed the fixed timeouts. Unsupported capabilities,
  workspace-loading errors, and LSP request failures stop extraction; there is no
  fallback parser or workaround pipeline.
