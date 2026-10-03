use std::process::{Command, Output};

fn query(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codegraph"))
        .args(args)
        .arg("--graph")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/navigation.json"
        ))
        // Queries must work without a language server or source repository.
        .env("RUST_ANALYZER", "/nonexistent/rust-analyzer")
        .output()
        .expect("start codegraph")
}

fn successful(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout).unwrap()
}

fn failed(output: Output) -> String {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn callers_are_direct_and_sorted() {
    assert_eq!(
        successful(query(&["callers", "rust::parse"])),
        "Callers of rust::parse [src/rust/mod.rs:1:4]:\n  main [src/main.rs:1:4]\n  util::helper [src/util.rs:1:4]\n"
    );
}

#[test]
fn callees_are_direct_and_sorted() {
    assert_eq!(
        successful(query(&["callees", "main"])),
        "Callees of main [src/main.rs:1:4]:\n  rust::parse [src/rust/mod.rs:1:4]\n  util::helper [src/util.rs:1:4]\n"
    );
}

#[test]
fn trace_finds_shortest_directed_path_despite_cycle() {
    assert_eq!(
        successful(query(&["trace", "main", "rust::helper"])),
        "main [src/main.rs:1:4]\n  -> rust::parse [src/rust/mod.rs:1:4]\n  -> rust::helper [src/rust/mod.rs:5:4]\n"
    );
    assert_eq!(
        successful(query(&["trace", "main", "main"])),
        "main [src/main.rs:1:4]\n"
    );
}

#[test]
fn ambiguous_names_list_candidates_and_exact_ids_disambiguate() {
    let error = failed(query(&["callers", "helper"]));
    assert!(error.contains("Ambiguous function 'helper'"));
    assert!(error.contains("rust::helper [src/rust/mod.rs:5:4]"));
    assert!(error.contains("util::helper [src/util.rs:1:4]"));
    assert_eq!(
        successful(query(&["callers", "src/util.rs:1:4"])),
        "Callers of util::helper [src/util.rs:1:4]:\n  main [src/main.rs:1:4]\n"
    );
}

#[test]
fn missing_functions_are_reported_for_neighbors_and_trace() {
    for args in [
        vec!["callers", "missing"],
        vec!["callees", "missing"],
        vec!["trace", "main", "missing"],
    ] {
        assert!(failed(query(&args)).contains("No function matches 'missing'"));
    }
}

#[test]
fn missing_paths_and_empty_neighbors_are_clear() {
    assert!(failed(query(&["trace", "main", "alone"])).contains("No call path"));
    for command in ["callers", "callees"] {
        assert_eq!(
            successful(query(&[command, "alone"])),
            format!("No direct {command} of alone [src/lib.rs:30:4] found in this graph.\n")
        );
    }
}
