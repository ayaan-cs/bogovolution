//! The lineage tree, rebuilt from the log.
//!
//! Nodes are versions of the rules file, named by the SHA-256 of their
//! content. Edges go from a parent version to a child version and carry the
//! hypothesis and the verdict. The log is the source of truth: nothing here
//! is stored on its own.

use crate::error::{Error, Result};
use crate::event::{
    Event, InitPayload, MutationProposedPayload, MutationRefusedPayload, Payload,
    RunRecordedPayload, VerdictKind, VerdictPayload,
};
use serde::Serialize;
use std::collections::HashMap;

/// Summary of one benchmark run recorded on a node.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunSummary {
    pub seq: u64,
    pub run_index: u32,
    pub passed: u32,
    pub total: u32,
    pub tokens_used: u64,
}

/// One version of the rules file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Node {
    /// SHA-256 hex of the content. Also the blob file name.
    pub hash: String,
    pub parent: Option<String>,
    /// 0 for the root.
    pub generation: u32,
    /// Sequence number of the event that created this node.
    pub created_seq: u64,
    /// Hypothesis of the edge into this node. `None` for the root.
    pub hypothesis: Option<String>,
    /// `None` for the root and for children that have no verdict yet.
    pub verdict: Option<VerdictKind>,
    pub runs: Vec<RunSummary>,
}

/// A parent to child link.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Edge {
    pub parent: String,
    pub child: String,
    pub hypothesis: String,
    /// `None` means no verdict yet (pending).
    pub verdict: Option<VerdictKind>,
    pub parent_median: Option<f64>,
    pub child_median: Option<f64>,
    pub paired_wins: Option<u32>,
    pub repeats: Option<u32>,
    pub note: Option<String>,
    /// Sequence number of the proposal event.
    pub seq: u64,
}

/// A mutation that was refused before it became a node.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Refusal {
    pub seq: u64,
    pub parent: String,
    pub hypothesis: Option<String>,
    pub reason: String,
}

/// Counts for the status command.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TreeSummary {
    pub nodes: usize,
    pub edges: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub noise: usize,
    pub pending: usize,
    pub refused: usize,
    pub runs: usize,
    pub tokens_used: u64,
    pub max_generation: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tree {
    pub rules_file: String,
    pub model: String,
    /// Hash of the root node.
    pub root: String,
    /// Hash of the newest accepted node, or the root if nothing was accepted.
    pub current: String,
    /// Nodes in the order they were created.
    pub nodes: Vec<Node>,
    /// Edges in the order they were proposed.
    pub edges: Vec<Edge>,
    pub refused: Vec<Refusal>,
    #[serde(skip)]
    node_index: HashMap<String, usize>,
    #[serde(skip)]
    edge_index: HashMap<String, usize>,
}

fn bad(msg: impl Into<String>) -> Error {
    Error::InvalidLineage(msg.into())
}

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(12)]
}

impl Tree {
    /// Rebuild the tree from a list of events (already chain-verified).
    pub fn from_events(events: &[Event]) -> Result<Tree> {
        let mut tree: Option<Tree> = None;
        for event in events {
            let payload = Payload::from_event(event).map_err(|e| {
                bad(format!(
                    "event {} ({}) has an unreadable payload: {e}",
                    event.seq,
                    event.kind.as_str()
                ))
            })?;
            match (&mut tree, payload) {
                (None, Payload::Init(init)) => {
                    if event.seq != 0 {
                        return Err(bad("init must be the first event"));
                    }
                    tree = Some(Tree::new(&init));
                }
                (None, _) => return Err(bad("the first event must be init")),
                (Some(_), Payload::Init(_)) => {
                    return Err(bad(format!("event {} is a second init", event.seq)))
                }
                (Some(t), other) => t.apply(event.seq, &other)?,
            }
        }
        tree.ok_or_else(|| bad("the log has no events"))
    }

    fn new(init: &InitPayload) -> Tree {
        let root = Node {
            hash: init.root_hash.clone(),
            parent: None,
            generation: 0,
            created_seq: 0,
            hypothesis: None,
            verdict: None,
            runs: Vec::new(),
        };
        let mut node_index = HashMap::new();
        node_index.insert(root.hash.clone(), 0);
        Tree {
            rules_file: init.rules_file.clone(),
            model: init.model.clone(),
            root: init.root_hash.clone(),
            current: init.root_hash.clone(),
            nodes: vec![root],
            edges: Vec::new(),
            refused: Vec::new(),
            node_index,
            edge_index: HashMap::new(),
        }
    }

    /// Apply one non-init payload. Used both when rebuilding and when
    /// checking a new event before it is written.
    pub fn apply(&mut self, seq: u64, payload: &Payload) -> Result<()> {
        match payload {
            Payload::Init(_) => Err(bad("init can only be the first event")),
            Payload::MutationProposed(p) => self.apply_proposed(seq, p),
            Payload::MutationRefused(p) => self.apply_refused(seq, p),
            Payload::RunRecorded(p) => self.apply_run(seq, p),
            Payload::Verdict(p) => self.apply_verdict(p),
        }
    }

    fn apply_proposed(&mut self, seq: u64, p: &MutationProposedPayload) -> Result<()> {
        let parent_idx = *self
            .node_index
            .get(&p.parent)
            .ok_or_else(|| bad(format!("unknown parent {}", short(&p.parent))))?;
        if self.node_index.contains_key(&p.child) {
            return Err(bad(format!(
                "version {} already exists in the tree",
                short(&p.child)
            )));
        }
        if p.hypothesis.trim().is_empty() {
            return Err(bad("a mutation needs a hypothesis"));
        }
        let generation = self.nodes[parent_idx].generation + 1;
        self.nodes.push(Node {
            hash: p.child.clone(),
            parent: Some(p.parent.clone()),
            generation,
            created_seq: seq,
            hypothesis: Some(p.hypothesis.clone()),
            verdict: None,
            runs: Vec::new(),
        });
        self.node_index
            .insert(p.child.clone(), self.nodes.len() - 1);
        self.edges.push(Edge {
            parent: p.parent.clone(),
            child: p.child.clone(),
            hypothesis: p.hypothesis.clone(),
            verdict: None,
            parent_median: None,
            child_median: None,
            paired_wins: None,
            repeats: None,
            note: None,
            seq,
        });
        self.edge_index
            .insert(p.child.clone(), self.edges.len() - 1);
        Ok(())
    }

    fn apply_refused(&mut self, seq: u64, p: &MutationRefusedPayload) -> Result<()> {
        if !self.node_index.contains_key(&p.parent) {
            return Err(bad(format!("unknown parent {}", short(&p.parent))));
        }
        self.refused.push(Refusal {
            seq,
            parent: p.parent.clone(),
            hypothesis: p.hypothesis.clone(),
            reason: p.reason.clone(),
        });
        Ok(())
    }

    fn apply_run(&mut self, seq: u64, p: &RunRecordedPayload) -> Result<()> {
        let idx = *self
            .node_index
            .get(&p.node)
            .ok_or_else(|| bad(format!("run for unknown version {}", short(&p.node))))?;
        if p.passed > p.total {
            return Err(bad("a run cannot pass more tasks than it ran"));
        }
        self.nodes[idx].runs.push(RunSummary {
            seq,
            run_index: p.run_index,
            passed: p.passed,
            total: p.total,
            tokens_used: p.tokens_used,
        });
        Ok(())
    }

    fn apply_verdict(&mut self, p: &VerdictPayload) -> Result<()> {
        let edge_idx = *self
            .edge_index
            .get(&p.child)
            .ok_or_else(|| bad(format!("verdict for unknown version {}", short(&p.child))))?;
        let edge = &mut self.edges[edge_idx];
        if edge.parent != p.parent {
            return Err(bad(format!(
                "verdict names parent {} but {} descends from {}",
                short(&p.parent),
                short(&p.child),
                short(&edge.parent)
            )));
        }
        if edge.verdict.is_some() {
            return Err(bad(format!(
                "version {} already has a verdict",
                short(&p.child)
            )));
        }
        edge.verdict = Some(p.verdict);
        edge.parent_median = p.parent_median;
        edge.child_median = p.child_median;
        edge.paired_wins = p.paired_wins;
        edge.repeats = p.repeats;
        edge.note = p.note.clone();
        let node_idx = self.node_index[&p.child];
        self.nodes[node_idx].verdict = Some(p.verdict);
        if p.verdict == VerdictKind::Accepted {
            self.current = p.child.clone();
        }
        Ok(())
    }

    pub fn node(&self, hash: &str) -> Option<&Node> {
        self.node_index.get(hash).map(|&i| &self.nodes[i])
    }

    pub fn children_of(&self, hash: &str) -> Vec<&Node> {
        self.nodes
            .iter()
            .filter(|n| n.parent.as_deref() == Some(hash))
            .collect()
    }

    pub fn summary(&self) -> TreeSummary {
        let count = |v: VerdictKind| self.edges.iter().filter(|e| e.verdict == Some(v)).count();
        TreeSummary {
            nodes: self.nodes.len(),
            edges: self.edges.len(),
            accepted: count(VerdictKind::Accepted),
            rejected: count(VerdictKind::Rejected),
            noise: count(VerdictKind::Noise),
            pending: self.edges.iter().filter(|e| e.verdict.is_none()).count(),
            refused: self.refused.len(),
            runs: self.nodes.iter().map(|n| n.runs.len()).sum(),
            tokens_used: self
                .nodes
                .iter()
                .flat_map(|n| n.runs.iter())
                .map(|r| r.tokens_used)
                .sum(),
            max_generation: self.nodes.iter().map(|n| n.generation).max().unwrap_or(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::Log;
    use tempfile::tempdir;

    fn h(c: char) -> String {
        c.to_string().repeat(64)
    }

    fn init() -> Payload {
        Payload::Init(InitPayload {
            rules_file: "AGENTS.md".into(),
            model: "m".into(),
            root_hash: h('a'),
            tool_version: "0.1.0".into(),
        })
    }

    fn proposed(parent: char, child: char, hyp: &str) -> Payload {
        Payload::MutationProposed(MutationProposedPayload {
            parent: h(parent),
            child: h(child),
            hypothesis: hyp.into(),
        })
    }

    fn verdict(parent: char, child: char, v: VerdictKind) -> Payload {
        Payload::Verdict(VerdictPayload {
            parent: h(parent),
            child: h(child),
            verdict: v,
            parent_median: Some(0.4),
            child_median: Some(0.6),
            paired_wins: Some(4),
            repeats: Some(5),
            note: None,
        })
    }

    fn build(payloads: &[Payload]) -> Result<Tree> {
        let dir = tempdir().unwrap();
        let log = Log::new(dir.path());
        for p in payloads {
            log.append_at(p, "2026-01-01T00:00:00Z").unwrap();
        }
        Tree::from_events(&log.read_events().unwrap())
    }

    #[test]
    fn rebuilds_nodes_edges_and_verdicts() {
        let tree = build(&[
            init(),
            proposed('a', 'b', "be terse"),
            Payload::RunRecorded(RunRecordedPayload {
                node: h('b'),
                run_index: 0,
                passed: 3,
                total: 5,
                tokens_used: 1000,
            }),
            verdict('a', 'b', VerdictKind::Accepted),
            proposed('b', 'c', "add test hint"),
            verdict('b', 'c', VerdictKind::Noise),
            proposed('b', 'd', "ban globals"),
            verdict('b', 'd', VerdictKind::Rejected),
            Payload::MutationRefused(MutationRefusedPayload {
                parent: h('b'),
                hypothesis: Some("rewrite everything".into()),
                reason: "too large".into(),
            }),
        ])
        .unwrap();

        assert_eq!(tree.root, h('a'));
        assert_eq!(tree.current, h('b'));
        assert_eq!(tree.nodes.len(), 4);
        assert_eq!(tree.node(&h('c')).unwrap().generation, 2);
        assert_eq!(
            tree.node(&h('b')).unwrap().verdict,
            Some(VerdictKind::Accepted)
        );
        assert_eq!(tree.children_of(&h('b')).len(), 2);
        let s = tree.summary();
        assert_eq!(s.edges, 3);
        assert_eq!(s.accepted, 1);
        assert_eq!(s.rejected, 1);
        assert_eq!(s.noise, 1);
        assert_eq!(s.pending, 0);
        assert_eq!(s.refused, 1);
        assert_eq!(s.runs, 1);
        assert_eq!(s.tokens_used, 1000);
        assert_eq!(s.max_generation, 2);
        let edge = &tree.edges[0];
        assert_eq!(edge.hypothesis, "be terse");
        assert_eq!(edge.paired_wins, Some(4));
    }

    #[test]
    fn pending_child_has_no_verdict() {
        let tree = build(&[init(), proposed('a', 'b', "x")]).unwrap();
        assert_eq!(tree.summary().pending, 1);
        assert_eq!(tree.current, h('a'));
    }

    #[test]
    fn rejects_unknown_parent() {
        let err = build(&[init(), proposed('z', 'b', "x")]).unwrap_err();
        assert!(matches!(err, Error::InvalidLineage(_)));
    }

    #[test]
    fn rejects_duplicate_version() {
        let err = build(&[init(), proposed('a', 'b', "x"), proposed('a', 'b', "y")]).unwrap_err();
        assert!(matches!(err, Error::InvalidLineage(_)));
    }

    #[test]
    fn rejects_second_verdict_and_second_init() {
        let err = build(&[
            init(),
            proposed('a', 'b', "x"),
            verdict('a', 'b', VerdictKind::Accepted),
            verdict('a', 'b', VerdictKind::Rejected),
        ])
        .unwrap_err();
        assert!(matches!(err, Error::InvalidLineage(_)));
        assert!(build(&[init(), init()]).is_err());
    }

    #[test]
    fn rejects_log_that_does_not_start_with_init() {
        assert!(build(&[proposed('a', 'b', "x")]).is_err());
    }

    #[test]
    fn rejects_empty_hypothesis() {
        assert!(build(&[init(), proposed('a', 'b', "  ")]).is_err());
    }
}
