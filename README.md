# codegraph

A small experimental Rust CLI that prints a repository-local function-call graph
as JSON. Rust syntax and call resolution come from rust-analyzer over LSP. There
is no Rust parser in this project, database, visualization, or AI integration.

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
   and Method symbols, then request `textDocument/prepareCallHierarchy` at each
   symbol's selection position. Preserve the returned call-hierarchy items.
5. Request `callHierarchy/outgoingCalls` for each discovered function, using its
   returned item. Retain function targets under the repository root.
6. Deduplicate and sort nodes/edges by ID, send `shutdown`/`exit`, and reap the
   server process. Startup and each request have a 60-second timeout.

Nodes have `id` and `name`; edges have `from` and `to` node IDs. IDs use relative
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
    { "id": "src/main.rs:10:4", "name": "save" },
    { "id": "src/main.rs:12:4", "name": "helper" },
    { "id": "src/main.rs:1:4", "name": "main" },
    { "id": "src/main.rs:6:4", "name": "parse" }
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
cargo test --locked --target-dir target
cargo fmt --check
cargo clippy --locked --target-dir target --all-targets -- -D warnings
```

Set `RUST_ANALYZER` for tests as well if the executable is outside PATH. The
integration test requires a real server and is never silently skipped: it checks
exactly four unique function nodes and the three known edges, including no extra
edges or dangling endpoints. Other tests check CLI usage and LSP byte framing,
including Unicode and truncated messages.

Validated locally with Rust 1.98.1 and rust-analyzer 0.3.3065-standalone. That host
has no `rust-src`; rust-analyzer warns about missing standard-library sources, but
the fixture's local calls resolve and all tests pass. For standard-library type
information, install `rust-src` rather than assuming that result generalizes.

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
  and unresolved types can limit results. Only the basic fixture is verified.
- File discovery is a directory walk, not a Cargo source inventory. Unlinked files
  may yield no semantic functions; unresolved prepared symbols are skipped with
  a warning. Cargo members/sources outside the supplied root are not scanned.
- Large workspaces can exceed the fixed timeouts. Unsupported capabilities,
  workspace-loading errors, and LSP request failures stop extraction; there is no
  fallback parser or workaround pipeline.
