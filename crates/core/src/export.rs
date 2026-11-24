//! `export`: write the whole lineage as one JSON file for the static viewer.
//!
//! The format is described by `schema/export.v1.json` in the repo. The viewer
//! needs nothing else: not the log, not the blobs, not the config. The export
//! has no timestamps and no paths, so the same lineage always exports to the
//! same bytes (the chain head hash does depend on the log's timestamps).

use crate::config::Config;
use crate::error::{Error, Result};
use crate::event::VerdictKind;
use crate::repo::Repo;
use crate::tree::{Node, Tree, TreeSummary};
use crate::verdict::{median, pair_win, Pair};
use serde::Serialize;

/// Version of the export format. Matches `schema/export.v1.json`.
pub const EXPORT_VERSION: u32 = 1;
pub const EXPORT_FORMAT: &str = "graft-export";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Generator {
    pub tool: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportProject {
    pub rules_file: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportChain {
    pub valid: bool,
    pub events: u64,
    pub head_seq: u64,
    pub head_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportConfig {
    pub repeats: u32,
    pub accept_threshold_pp: f64,
    pub min_paired_wins: u32,
    pub max_tokens_per_run: u64,
    pub max_change_bytes: usize,
    /// The held out gate. Only present when some run scored held out tasks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out_accept_threshold_pp: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out_min_paired_wins: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportRun {
    pub run_index: u32,
    pub seed: Option<u64>,
    pub passed: u32,
    pub total: u32,
    pub pass_rate: f64,
    pub tokens_used: u64,
    pub budget_exhausted: bool,
    /// Held out tasks of the run. `passed`, `total` and `pass_rate` then
    /// count train tasks only. Absent when there were no held out tasks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out_passed: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out_total: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out_pass_rate: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportNode {
    pub hash: String,
    pub short: String,
    pub parent: Option<String>,
    pub generation: u32,
    pub created_seq: u64,
    pub hypothesis: Option<String>,
    pub verdict: Option<VerdictKind>,
    /// `root`, `pending`, `accepted`, `rejected`, `noise` or `overfit`.
    pub status: String,
    pub is_current: bool,
    pub size_bytes: usize,
    pub content: String,
    pub diff: Option<String>,
    pub median_pass_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out_median_pass_rate: Option<f64>,
    pub runs: Vec<ExportRun>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportEdge {
    pub parent: String,
    pub child: String,
    pub seq: u64,
    pub hypothesis: String,
    pub verdict: Option<VerdictKind>,
    pub reason: Option<String>,
    pub parent_median: Option<f64>,
    pub child_median: Option<f64>,
    pub gain_pp: Option<f64>,
    pub paired_wins: Option<u32>,
    pub repeats: Option<u32>,
    pub pairs: Vec<Pair>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out_parent_median: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out_child_median: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out_gain_pp: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out_paired_wins: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportRefusal {
    pub seq: u64,
    pub parent: String,
    pub hypothesis: Option<String>,
    pub reason: String,
    pub seed: Option<u64>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeriesPoint {
    pub attempt: u32,
    pub seq: u64,
    /// `accepted`, `rejected`, `noise`, `overfit`, `pending` or `refused`.
    pub outcome: String,
    pub parent: String,
    pub child: Option<String>,
    pub hypothesis: Option<String>,
    pub parent_median: Option<f64>,
    pub child_median: Option<f64>,
    pub gain_pp: Option<f64>,
    pub current_after: String,
    pub current_median_after: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Export {
    pub format: String,
    pub version: u32,
    pub generator: Generator,
    pub project: ExportProject,
    pub chain: ExportChain,
    pub config: ExportConfig,
    pub root: String,
    pub current: String,
    pub summary: TreeSummary,
    pub nodes: Vec<ExportNode>,
    pub edges: Vec<ExportEdge>,
    pub refused: Vec<ExportRefusal>,
    pub series: Vec<SeriesPoint>,
}

fn short(hash: &str) -> String {
    hash[..hash.len().min(12)].to_string()
}

/// Median pass rate over a node's recorded runs.
pub fn node_median(node: &Node) -> Option<f64> {
    let rates: Vec<f64> = node
        .runs
        .iter()
        .filter(|r| r.total > 0)
        .map(|r| r.passed as f64 / r.total as f64)
        .collect();
    if rates.is_empty() {
        None
    } else {
        Some(median(&rates))
    }
}

/// Held out median pass rate over a node's recorded runs, when they scored
/// held out tasks.
pub fn node_held_out_median(node: &Node) -> Option<f64> {
    let rates: Vec<f64> = node
        .runs
        .iter()
        .filter_map(|r| match (r.held_out_passed, r.held_out_total) {
            (Some(p), Some(t)) if t > 0 => Some(p as f64 / t as f64),
            _ => None,
        })
        .collect();
    if rates.is_empty() {
        None
    } else {
        Some(median(&rates))
    }
}

/// Git style unified diff, for the viewer's diff panel.
pub fn unified_diff(file: &str, old: &str, new: &str) -> String {
    let body = similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{file}"), &format!("b/{file}"))
        .to_string();
    format!("diff --git a/{file} b/{file}\n{body}")
}

/// Pairs of runs (same run index, same seed) recorded for both versions.
fn pairs_for(parent: &Node, child: &Node) -> Vec<Pair> {
    let mut out = Vec::new();
    for c in child.runs.iter().filter(|r| r.total > 0) {
        if let Some(p) = parent
            .runs
            .iter()
            .find(|p| p.run_index == c.run_index && p.total > 0)
        {
            if p.seed.is_some() && c.seed.is_some() && p.seed != c.seed {
                continue;
            }
            let pr = p.passed as f64 / p.total as f64;
            let cr = c.passed as f64 / c.total as f64;
            out.push(Pair {
                run_index: c.run_index,
                parent_rate: pr,
                child_rate: cr,
                win: pair_win(pr, cr),
            });
        }
    }
    out.sort_by_key(|p| p.run_index);
    out
}

fn gain(parent: Option<f64>, child: Option<f64>) -> Option<f64> {
    match (parent, child) {
        (Some(p), Some(c)) => Some((c - p) * 100.0),
        _ => None,
    }
}

/// Build the series: one point per attempt, with the `current` version as it
/// stood once the attempt was settled.
fn build_series(tree: &Tree) -> Vec<SeriesPoint> {
    enum Item {
        Proposal(usize),
        Refusal(usize),
        Verdict(usize),
    }
    let mut timeline: Vec<(u64, Item)> = Vec::new();
    for (i, e) in tree.edges.iter().enumerate() {
        timeline.push((e.seq, Item::Proposal(i)));
        if let Some(vs) = e.verdict_seq {
            timeline.push((vs, Item::Verdict(i)));
        }
    }
    for (i, r) in tree.refused.iter().enumerate() {
        timeline.push((r.seq, Item::Refusal(i)));
    }
    timeline.sort_by_key(|(seq, _)| *seq);

    let median_of = |hash: &str| tree.node(hash).and_then(node_median);
    let mut current = tree.root.clone();
    let mut points: Vec<SeriesPoint> = Vec::new();
    let mut edge_point: Vec<Option<usize>> = vec![None; tree.edges.len()];

    for (seq, item) in timeline {
        match item {
            Item::Proposal(i) => {
                let e = &tree.edges[i];
                let parent_median = e.parent_median.or_else(|| median_of(&e.parent));
                let child_median = e.child_median.or_else(|| median_of(&e.child));
                edge_point[i] = Some(points.len());
                points.push(SeriesPoint {
                    attempt: points.len() as u32 + 1,
                    seq,
                    outcome: e
                        .verdict
                        .map(|v| v.as_str())
                        .unwrap_or("pending")
                        .to_string(),
                    parent: e.parent.clone(),
                    child: Some(e.child.clone()),
                    hypothesis: Some(e.hypothesis.clone()),
                    parent_median,
                    child_median,
                    gain_pp: gain(parent_median, child_median),
                    current_after: current.clone(),
                    current_median_after: None,
                });
            }
            Item::Refusal(i) => {
                let r = &tree.refused[i];
                points.push(SeriesPoint {
                    attempt: points.len() as u32 + 1,
                    seq,
                    outcome: "refused".into(),
                    parent: r.parent.clone(),
                    child: None,
                    hypothesis: r.hypothesis.clone(),
                    parent_median: None,
                    child_median: None,
                    gain_pp: None,
                    current_after: current.clone(),
                    current_median_after: None,
                });
            }
            Item::Verdict(i) => {
                let e = &tree.edges[i];
                if e.verdict == Some(VerdictKind::Accepted) {
                    current = e.child.clone();
                }
                if let Some(pi) = edge_point[i] {
                    points[pi].current_after = current.clone();
                }
            }
        }
    }
    for p in &mut points {
        p.current_median_after = median_of(&p.current_after);
    }
    points
}

/// Build the export from a verified project. Fails if the log does not verify.
pub fn build_export(repo: &Repo) -> Result<Export> {
    let verify = repo.verify()?;
    if !verify.valid {
        return Err(Error::LogInvalid(
            "cannot export a log that fails verification. Run `verify-log` for details".into(),
        ));
    }
    let events = repo.events()?;
    let tree = Tree::from_events(&events)?;
    let config: Config = repo.config()?;
    let head = events
        .last()
        .ok_or_else(|| Error::InvalidLineage("the log has no events".into()))?;

    let has_held_out = tree
        .nodes
        .iter()
        .any(|n| n.runs.iter().any(|r| r.held_out_total.is_some()));
    // The gate that applies, which depends on the size of the held out set.
    let held_out_tasks = tree
        .nodes
        .iter()
        .flat_map(|n| n.runs.iter())
        .filter_map(|r| r.held_out_total)
        .max()
        .unwrap_or(0);
    let mut contents = std::collections::HashMap::new();
    for n in &tree.nodes {
        let bytes = repo.store().get_blob(&n.hash)?;
        contents.insert(n.hash.clone(), String::from_utf8_lossy(&bytes).to_string());
    }

    let nodes: Vec<ExportNode> = tree
        .nodes
        .iter()
        .map(|n| {
            let content = contents[&n.hash].clone();
            let diff = n
                .parent
                .as_ref()
                .map(|p| unified_diff(&tree.rules_file, &contents[p], &content));
            let status = match (&n.parent, n.verdict) {
                (None, _) => "root",
                (Some(_), None) => "pending",
                (Some(_), Some(v)) => v.as_str(),
            };
            ExportNode {
                hash: n.hash.clone(),
                short: short(&n.hash),
                parent: n.parent.clone(),
                generation: n.generation,
                created_seq: n.created_seq,
                hypothesis: n.hypothesis.clone(),
                verdict: n.verdict,
                status: status.to_string(),
                is_current: n.hash == tree.current,
                size_bytes: content.len(),
                content,
                diff,
                median_pass_rate: node_median(n),
                held_out_median_pass_rate: node_held_out_median(n),
                runs: n
                    .runs
                    .iter()
                    .map(|r| ExportRun {
                        run_index: r.run_index,
                        seed: r.seed,
                        passed: r.passed,
                        total: r.total,
                        pass_rate: if r.total == 0 {
                            0.0
                        } else {
                            r.passed as f64 / r.total as f64
                        },
                        tokens_used: r.tokens_used,
                        budget_exhausted: r.budget_exhausted,
                        held_out_passed: r.held_out_passed,
                        held_out_total: r.held_out_total,
                        held_out_pass_rate: match (r.held_out_passed, r.held_out_total) {
                            (Some(p), Some(t)) if t > 0 => Some(p as f64 / t as f64),
                            (Some(_), Some(_)) => Some(0.0),
                            _ => None,
                        },
                    })
                    .collect(),
            }
        })
        .collect();

    let edges: Vec<ExportEdge> = tree
        .edges
        .iter()
        .map(|e| {
            let pairs = match (tree.node(&e.parent), tree.node(&e.child)) {
                (Some(p), Some(c)) => pairs_for(p, c),
                _ => Vec::new(),
            };
            ExportEdge {
                parent: e.parent.clone(),
                child: e.child.clone(),
                seq: e.seq,
                hypothesis: e.hypothesis.clone(),
                verdict: e.verdict,
                reason: e.note.clone(),
                parent_median: e.parent_median,
                child_median: e.child_median,
                gain_pp: gain(e.parent_median, e.child_median),
                paired_wins: e.paired_wins,
                repeats: e.repeats,
                pairs,
                held_out_parent_median: e.held_out_parent_median,
                held_out_child_median: e.held_out_child_median,
                held_out_gain_pp: gain(e.held_out_parent_median, e.held_out_child_median),
                held_out_paired_wins: e.held_out_paired_wins,
            }
        })
        .collect();

    let refused = tree
        .refused
        .iter()
        .map(|r| ExportRefusal {
            seq: r.seq,
            parent: r.parent.clone(),
            hypothesis: r.hypothesis.clone(),
            reason: r.reason.clone(),
            seed: r.seed,
            provider: r.provider.clone(),
        })
        .collect();

    Ok(Export {
        format: EXPORT_FORMAT.to_string(),
        version: EXPORT_VERSION,
        generator: Generator {
            tool: "graft".into(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        project: ExportProject {
            rules_file: tree.rules_file.clone(),
            model: tree.model.clone(),
        },
        chain: ExportChain {
            valid: true,
            events: events.len() as u64,
            head_seq: head.seq,
            head_hash: head.hash.clone(),
        },
        config: ExportConfig {
            repeats: config.evaluation.repeats,
            accept_threshold_pp: config.evaluation.accept_threshold_pp,
            min_paired_wins: config.evaluation.min_paired_wins,
            max_tokens_per_run: config.budget.max_tokens_per_run,
            max_change_bytes: config.mutation.max_change_bytes,
            held_out_accept_threshold_pp: has_held_out
                .then_some(config.evaluation.held_out_threshold_for(held_out_tasks)),
            held_out_min_paired_wins: has_held_out
                .then_some(config.evaluation.held_out_min_paired_wins),
        },
        root: tree.root.clone(),
        current: tree.current.clone(),
        summary: tree.summary(),
        nodes,
        edges,
        refused,
        series: build_series(&tree),
    })
}

/// The export as pretty JSON with a trailing newline.
pub fn to_json(export: &Export) -> String {
    let mut s = serde_json::to_string_pretty(export).expect("export encodes");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    #[test]
    fn diff_is_git_style_and_empty_free() {
        let d = unified_diff("AGENTS.md", "one\ntwo\n", "one\ntwo\nthree\n");
        assert!(
            d.starts_with("diff --git a/AGENTS.md b/AGENTS.md\n--- a/AGENTS.md\n+++ b/AGENTS.md\n")
        );
        assert!(d.contains("+three"));
        assert!(!d.contains("-two"));
    }

    #[test]
    fn node_median_ignores_empty_runs() {
        let t = tree(&[init(), run('a', 0, 2), run('a', 1, 8), run('a', 2, 6)]);
        assert_eq!(node_median(t.node(&t.root).unwrap()), Some(0.6));
        let none = tree(&[init()]);
        assert_eq!(node_median(none.node(&none.root).unwrap()), None);
    }

    #[test]
    fn pairs_match_by_run_index_and_seed() {
        let t = tree(&[
            init(),
            run('a', 0, 3),
            run('a', 1, 5),
            proposed('a', 'b', "x"),
            run('b', 0, 6),
            run('b', 1, 5),
            run('b', 2, 9),
        ]);
        let pairs = pairs_for(t.node(&h('a')).unwrap(), t.node(&h('b')).unwrap());
        assert_eq!(pairs.len(), 2, "run 2 has no parent run");
        assert!(pairs[0].win);
        assert!(!pairs[1].win, "ties are not wins");
        assert_eq!((pairs[0].parent_rate, pairs[0].child_rate), (0.3, 0.6));
    }

    #[test]
    fn series_tracks_current_through_a_late_verdict() {
        // Both children are proposed before either verdict lands.
        let t = tree(&[
            init(),
            run('a', 0, 3),
            proposed('a', 'b', "first"),
            proposed('a', 'c', "second"),
            run('b', 0, 8),
            verdict('a', 'b', VerdictKind::Accepted, 0.3, 0.8, 5),
            verdict('a', 'c', VerdictKind::Rejected, 0.3, 0.1, 0),
            refused('b', Some("third"), "nope"),
            proposed('b', 'd', "fourth"),
        ]);
        let s = build_series(&t);
        let outcomes: Vec<&str> = s.iter().map(|p| p.outcome.as_str()).collect();
        assert_eq!(outcomes, ["accepted", "rejected", "refused", "pending"]);
        assert_eq!(s[0].attempt, 1);
        assert_eq!(s[3].attempt, 4);
        assert_eq!(s[0].current_after, h('b'));
        assert_eq!(
            s[1].current_after,
            h('b'),
            "accepted b landed before c was judged"
        );
        assert_eq!(s[2].current_after, h('b'));
        assert_eq!(s[3].current_after, h('b'));
        assert_eq!(s[0].current_median_after, Some(0.8));
        assert!((s[0].gain_pp.unwrap() - 50.0).abs() < 1e-9);
        assert!(s[2].child.is_none() && s[2].gain_pp.is_none());
        assert_eq!(s[3].child_median, None);
    }

    #[test]
    fn short_hash_is_twelve_chars() {
        assert_eq!(short(&h('f')).len(), 12);
    }
}
