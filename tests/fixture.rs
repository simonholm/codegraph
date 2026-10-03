use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    process::Command,
};

#[test]
fn extracts_exact_fixture_graph_through_real_rust_analyzer() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple-rust");
    let output = Command::new(env!("CARGO_BIN_EXE_codegraph"))
        .arg(fixture)
        .output()
        .expect("start codegraph");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Parse all stdout, so logging mixed into the JSON also fails this test.
    let graph: Value = serde_json::from_slice(&output.stdout).expect("valid JSON on stdout");
    let nodes = graph["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 4);
    let names: BTreeSet<_> = nodes
        .iter()
        .map(|node| node["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, BTreeSet::from(["main", "parse", "save", "helper"]));
    for node in nodes {
        assert_eq!(node["symbol_path"], node["name"]);
    }
    let ids: BTreeMap<_, _> = nodes
        .iter()
        .map(|node| (node["id"].as_str().unwrap(), node["name"].as_str().unwrap()))
        .collect();
    assert_eq!(ids.len(), 4, "node IDs must be unique");
    let edges = graph["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 3, "no extra or repeated edges");
    let calls: BTreeSet<_> = edges
        .iter()
        .map(|edge| {
            (
                ids[edge["from"].as_str().unwrap()],
                ids[edge["to"].as_str().unwrap()],
            )
        })
        .collect();
    assert_eq!(
        calls,
        BTreeSet::from([("main", "parse"), ("main", "save"), ("parse", "helper")])
    );
}

#[test]
fn rejects_missing_argument_before_starting_server() {
    let output = Command::new(env!("CARGO_BIN_EXE_codegraph"))
        .output()
        .expect("start codegraph");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage: codegraph"));
}
