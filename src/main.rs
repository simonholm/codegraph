mod lsp;

use anyhow::{Context, Result, ensure};
use lsp::Lsp;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use url::Url;

#[derive(Serialize)]
struct Node {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct Edge {
    from: String,
    to: String,
}

#[derive(Serialize)]
struct Graph {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("codegraph: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let root = args
        .next()
        .context("Usage: codegraph <rust-repository-path>")?;
    ensure!(
        args.next().is_none(),
        "Usage: codegraph <rust-repository-path>"
    );
    let root = fs::canonicalize(root).context("Cannot resolve repository path")?;
    ensure!(
        root.join("Cargo.toml").is_file(),
        "Repository root must contain Cargo.toml"
    );
    let mut files = Vec::new();
    rust_files(&root, &mut files)?;
    files.sort();
    let uri =
        Url::from_directory_path(&root).map_err(|_| anyhow::anyhow!("Invalid repository path"))?;
    let mut lsp = Lsp::start(&root, uri.as_str())?;
    let graph = extract(&mut lsp, &root, &files)?;
    lsp.shutdown()?;
    serde_json::to_writer_pretty(std::io::stdout().lock(), &graph)?;
    println!();
    Ok(())
}

fn rust_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with('.')
                && !matches!(
                    name.as_ref(),
                    "target"
                        | "node_modules"
                        | "dist"
                        | "build"
                        | "tmp"
                        | "caches"
                        | "logs"
                        | "snap"
                        | "venv"
                )
            {
                rust_files(&path, files)?;
            }
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn function_symbols<'a>(symbols: &'a Value, functions: &mut Vec<&'a Value>) {
    if let Some(symbols) = symbols.as_array() {
        for symbol in symbols {
            // LSP Function (12) and Method (6) are both Rust functions.
            if symbol["kind"] == 12 || symbol["kind"] == 6 {
                functions.push(symbol);
            }
            function_symbols(&symbol["children"], functions);
        }
    }
}

fn node(root: &Path, item: &Value) -> Result<Option<Node>> {
    if item["kind"] != 12 && item["kind"] != 6 {
        return Ok(None);
    }
    let uri = Url::parse(item["uri"].as_str().context("Call item has no URI")?)?;
    let path = uri
        .to_file_path()
        .map_err(|_| anyhow::anyhow!("Call item URI is not a file"))?;
    let Ok(relative) = path.strip_prefix(root) else {
        // Dependencies and the standard library are outside the v0 graph.
        return Ok(None);
    };
    let start = &item["selectionRange"]["start"];
    let line = start["line"].as_u64().context("Call item has no line")? + 1;
    let column = start["character"]
        .as_u64()
        .context("Call item has no column")?
        + 1;
    Ok(Some(Node {
        id: format!("{}:{line}:{column}", relative.display()),
        name: item["name"]
            .as_str()
            .context("Call item has no name")?
            .to_owned(),
    }))
}

fn extract(lsp: &mut Lsp, root: &Path, files: &[PathBuf]) -> Result<Graph> {
    let mut items = BTreeMap::new();
    let mut nodes = BTreeMap::new();
    for file in files {
        let uri = Url::from_file_path(file).map_err(|_| anyhow::anyhow!("Invalid source path"))?;
        let symbols = lsp.request(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri.as_str()}}),
        )?;
        let mut functions = Vec::new();
        function_symbols(&symbols, &mut functions);
        for symbol in functions {
            let prepared = lsp.request(
                "textDocument/prepareCallHierarchy",
                json!({
                    "textDocument": {"uri": uri.as_str()}, "position": symbol["selectionRange"]["start"]
                }),
            )?;
            if prepared.as_array().is_none_or(|items| items.is_empty()) {
                eprintln!(
                    "Skipping unresolved function {} in {}",
                    symbol["name"],
                    file.display()
                );
            }
            for item in prepared.as_array().into_iter().flatten() {
                if let Some(node) = node(root, item)? {
                    items.insert(node.id.clone(), item.clone());
                    nodes.insert(node.id.clone(), node);
                }
            }
        }
    }
    let mut edges = BTreeSet::new();
    for (from, item) in items {
        let calls = lsp.request("callHierarchy/outgoingCalls", json!({"item": item}))?;
        ensure!(
            !calls.is_null(),
            "rust-analyzer returned no call hierarchy for {from}"
        );
        for call in calls
            .as_array()
            .context("Outgoing calls must be an array")?
        {
            if let Some(to) = node(root, &call["to"])? {
                edges.insert((from.clone(), to.id.clone()));
                nodes.insert(to.id.clone(), to);
            }
        }
    }
    Ok(Graph {
        nodes: nodes.into_values().collect(),
        edges: edges
            .into_iter()
            .map(|(from, to)| Edge { from, to })
            .collect(),
    })
}
