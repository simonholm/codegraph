use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, VecDeque};

#[derive(Deserialize, Serialize)]
pub struct Node {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize, Serialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
}

#[derive(Deserialize, Serialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl Node {
    // File-based aliases work with existing v0 JSON; they are not Rust item paths.
    fn alias(&self) -> String {
        let file = self.id.rsplitn(3, ':').nth(2).unwrap_or(&self.id);
        let file = file.strip_prefix("src/").unwrap_or(file);
        let file = file.strip_suffix(".rs").unwrap_or(file);
        let mut parts: Vec<_> = file.split('/').collect();
        if parts.last() == Some(&"mod") || (parts.len() == 1 && matches!(parts[0], "lib" | "main"))
        {
            parts.pop();
        }
        parts.push(&self.name);
        parts.join("::")
    }

    fn label(&self) -> String {
        format!("{} [{}]", self.alias(), self.id)
    }
}

impl Graph {
    fn resolve(&self, selector: &str) -> Result<&Node> {
        if let Some(node) = self.nodes.iter().find(|node| node.id == selector) {
            return Ok(node);
        }
        let mut matches: Vec<_> = self
            .nodes
            .iter()
            .filter(|node| node.name == selector || node.alias() == selector)
            .collect();
        matches.sort_by_key(|node| node.label());
        match matches.as_slice() {
            [node] => Ok(node),
            [] => bail!("No function matches '{selector}' in this graph."),
            _ => bail!(
                "Ambiguous function '{selector}'. Matching candidates:\n{}",
                matches
                    .iter()
                    .map(|node| format!("  {}", node.label()))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        }
    }

    pub fn query(&self, command: &str, from: &str, to: Option<&str>) -> Result<String> {
        let nodes: HashMap<_, _> = self
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect();
        ensure!(
            nodes.len() == self.nodes.len(),
            "Graph contains duplicate node IDs"
        );
        ensure!(
            self.edges
                .iter()
                .all(|edge| nodes.contains_key(edge.from.as_str())
                    && nodes.contains_key(edge.to.as_str())),
            "Graph contains an edge with a missing endpoint"
        );
        let source = self.resolve(from)?;
        match command {
            "callers" | "callees" => {
                let callers = command == "callers";
                let neighbors: BTreeSet<_> = self
                    .edges
                    .iter()
                    .filter_map(|edge| {
                        let (anchor, neighbor) = if callers {
                            (&edge.to, &edge.from)
                        } else {
                            (&edge.from, &edge.to)
                        };
                        (anchor == &source.id).then(|| nodes[neighbor.as_str()].label())
                    })
                    .collect();
                if neighbors.is_empty() {
                    return Ok(format!(
                        "No direct {command} of {} found in this graph.\n",
                        source.label()
                    ));
                }
                Ok(format!(
                    "{} of {}:\n{}\n",
                    if callers { "Callers" } else { "Callees" },
                    source.label(),
                    neighbors
                        .into_iter()
                        .map(|label| format!("  {label}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ))
            }
            "trace" => {
                let target = self.resolve(to.context("trace requires a destination function")?)?;
                let mut outgoing: HashMap<&str, BTreeSet<&str>> = HashMap::new();
                for edge in &self.edges {
                    outgoing.entry(&edge.from).or_default().insert(&edge.to);
                }
                let mut parents = HashMap::from([(source.id.as_str(), None)]);
                let mut queue = VecDeque::from([source.id.as_str()]);
                while let Some(id) = queue.pop_front() {
                    if id == target.id {
                        let mut path = Vec::new();
                        let mut current = Some(id);
                        while let Some(id) = current {
                            path.push(nodes[id].label());
                            current = parents[id];
                        }
                        path.reverse();
                        return Ok(format!("{}\n", path.join("\n  -> ")));
                    }
                    for next in outgoing.get(id).into_iter().flatten() {
                        if !parents.contains_key(next) {
                            parents.insert(*next, Some(id));
                            queue.push_back(next);
                        }
                    }
                }
                bail!(
                    "No call path from {} to {} exists in this graph.",
                    source.label(),
                    target.label()
                )
            }
            _ => bail!("Unknown query '{command}'"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Graph {
        serde_json::from_str(include_str!("../tests/fixtures/navigation.json")).unwrap()
    }

    #[test]
    fn file_alias_collision_stays_ambiguous() {
        let mut graph = fixture();
        graph.nodes.push(Node {
            id: "src/rust/mod.rs:10:4".into(),
            name: "helper".into(),
        });
        let error = graph
            .query("callers", "rust::helper", None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Ambiguous function"));
        assert!(error.contains("src/rust/mod.rs:5:4"));
        assert!(error.contains("src/rust/mod.rs:10:4"));
    }

    #[test]
    fn rejects_duplicate_ids_and_dangling_edges() {
        let mut graph = fixture();
        graph.nodes.push(Node {
            id: "src/main.rs:1:4".into(),
            name: "duplicate".into(),
        });
        assert!(
            graph
                .query("callees", "main", None)
                .unwrap_err()
                .to_string()
                .contains("duplicate node IDs")
        );
        let mut graph = fixture();
        graph.edges.push(Edge {
            from: "src/main.rs:1:4".into(),
            to: "missing-id".into(),
        });
        assert!(
            graph
                .query("callees", "main", None)
                .unwrap_err()
                .to_string()
                .contains("missing endpoint")
        );
    }
}
