use serde_json::Value;
use std::{collections::BTreeMap, path::Path, process::Command};

#[test]
fn same_file_functions_and_methods_keep_server_context_and_exact_ids() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/identities-rust");
    let output = Command::new(env!("CARGO_BIN_EXE_codegraph"))
        .arg(&fixture)
        .output()
        .expect("start codegraph");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let graph: Value = serde_json::from_slice(&output.stdout).unwrap();
    let nodes: BTreeMap<_, _> = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            assert_eq!(node["name"], "search");
            (
                node["symbol_path"].as_str().unwrap(),
                node["id"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        nodes,
        BTreeMap::from([
            ("Adapter::search", "src/lib.rs:2:8"),
            ("Other::search", "src/lib.rs:6:8"),
            ("impl Recall::search", "src/lib.rs:12:12"),
            ("impl Adapter for Recall::search", "src/lib.rs:18:8"),
            ("impl Other for Recall::search", "src/lib.rs:22:8"),
            ("impl Adapter for Generic<T>::search", "src/lib.rs:28:8"),
            ("nested::search", "src/lib.rs:32:12"),
            ("search", "src/lib.rs:37:8"),
        ])
    );
    assert_eq!(graph["nodes"].as_array().unwrap().len(), nodes.len());
    let edges: Vec<_> = graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| (edge["from"].as_str().unwrap(), edge["to"].as_str().unwrap()))
        .collect();
    assert_eq!(
        edges,
        vec![
            (nodes["impl Recall::search"], nodes["Adapter::search"]),
            (nodes["nested::search"], nodes["impl Recall::search"]),
            (nodes["search"], nodes["nested::search"]),
        ]
    );
}
