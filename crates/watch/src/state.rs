//! The state model of `watch`: events in, tree rows, run progress, verdict feed
//! and details out. Nothing here draws anything or touches the terminal, so it
//! is plain unit tested data in and data out.

use graft_rules_core::event::{Event, Payload, VerdictKind};
use graft_rules_core::export::node_median;
use graft_rules_core::tree::{Node, Refusal, Tree};
use std::collections::HashMap;

/// The few config values the view needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConfigView {
    pub repeats: u32,
    pub accept_threshold_pp: f64,
    pub min_paired_wins: u32,
    pub max_tokens_per_run: u64,
}

impl Default for ConfigView {
    fn default() -> Self {
        ConfigView {
            repeats: 5,
            accept_threshold_pp: 10.0,
            min_paired_wins: 4,
            max_tokens_per_run: 200_000,
        }
    }
}

/// Whether the hash chain checked out on the latest read.
#[derive(Debug, Clone, PartialEq)]
pub struct ChainStatus {
    pub valid: bool,
    /// Events that passed the chain check.
    pub events: u64,
    /// What is wrong, in plain words. Empty when valid.
    pub problems: Vec<String>,
}

impl ChainStatus {
    pub fn ok(events: u64) -> ChainStatus {
        ChainStatus {
            valid: true,
            events,
            problems: Vec::new(),
        }
    }
}

/// A row in the tree pane is a version or a refused mutation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RowId {
    Version(String),
    Refused(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowStatus {
    Root,
    Pending,
    Accepted,
    Rejected,
    Noise,
    Overfit,
    Refused,
}

impl RowStatus {
    pub fn label(self) -> &'static str {
        match self {
            RowStatus::Root => "root",
            RowStatus::Pending => "pending",
            RowStatus::Accepted => "accepted",
            RowStatus::Rejected => "rejected",
            RowStatus::Noise => "noise",
            RowStatus::Overfit => "overfit",
            RowStatus::Refused => "refused",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: RowId,
    /// Log sequence number of the event that created the row.
    pub seq: u64,
    pub depth: usize,
    /// Tree guide characters in front of the row.
    pub prefix: String,
    /// First seven characters of the hash. Empty for refused rows.
    pub short: String,
    pub status: RowStatus,
    pub is_current: bool,
    /// The hypothesis, or a note for the root.
    pub title: String,
    pub gain_pp: Option<f64>,
    pub median: Option<f64>,
}

/// Which version the benchmark is working on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Root,
    Parent,
    Child,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Root => "starting version",
            Role::Parent => "parent",
            Role::Child => "child",
        }
    }
}

/// Run level progress of the version that was benchmarked most recently.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub version: String,
    pub role: Role,
    pub hypothesis: Option<String>,
    pub runs_done: u32,
    pub repeats: u32,
    /// Pass rate of each recorded run, in run order, from 0 to 1.
    pub rates: Vec<f64>,
    /// The version is judged, so no more runs are expected for it.
    pub settled: bool,
}

impl Progress {
    /// Share of the runs that are done, from 0 to 1.
    pub fn ratio(&self) -> f64 {
        if self.repeats == 0 {
            0.0
        } else {
            (f64::from(self.runs_done) / f64::from(self.repeats)).min(1.0)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedKind {
    Verdict(VerdictKind),
    Refused,
}

/// One line of the verdict feed.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedItem {
    pub seq: u64,
    pub kind: FeedKind,
    pub target: RowId,
    pub short: String,
    pub hypothesis: String,
    pub gain_pp: Option<f64>,
    pub paired_wins: Option<u32>,
    pub repeats: Option<u32>,
    pub reason: String,
}

/// What the most recent event was.
#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    pub seq: u64,
    pub text: String,
}

/// Everything the details pane and overlay need about one row.
#[derive(Debug, Clone, PartialEq)]
pub struct Details {
    pub title: String,
    pub status: RowStatus,
    pub is_current: bool,
    pub fields: Vec<(String, String)>,
    pub hypothesis: Option<String>,
    pub reason: Option<String>,
    pub runs: Vec<String>,
    /// For a version, the hash and the parent hash, so the app can load a diff.
    pub hash: Option<String>,
    pub parent_hash: Option<String>,
}

/// The whole view model, rebuilt whenever the log changes.
#[derive(Debug, Clone)]
pub struct WatchState {
    pub chain: ChainStatus,
    pub config: ConfigView,
    pub rules_file: String,
    pub model: String,
    pub tree: Option<Tree>,
    pub rows: Vec<Row>,
    pub progress: Option<Progress>,
    /// Newest first.
    pub feed: Vec<FeedItem>,
    pub activity: Option<Activity>,
    pub tokens_total: u64,
    /// The most tokens any single run used.
    pub tokens_max_run: u64,
    pub accepted: usize,
    pub rejected: usize,
    pub noise: usize,
    pub overfit: usize,
    pub pending: usize,
    pub refused: usize,
}

pub fn short7(hash: &str) -> String {
    hash.chars().take(7).collect()
}

fn verdict_status(v: Option<VerdictKind>, is_root: bool) -> RowStatus {
    match (is_root, v) {
        (true, _) => RowStatus::Root,
        (false, None) => RowStatus::Pending,
        (false, Some(VerdictKind::Accepted)) => RowStatus::Accepted,
        (false, Some(VerdictKind::Rejected)) => RowStatus::Rejected,
        (false, Some(VerdictKind::Noise)) => RowStatus::Noise,
        (false, Some(VerdictKind::Overfit)) => RowStatus::Overfit,
    }
}

fn gain(parent: Option<f64>, child: Option<f64>) -> Option<f64> {
    match (parent, child) {
        (Some(p), Some(c)) => Some((c - p) * 100.0),
        _ => None,
    }
}

enum Kid<'a> {
    Node(&'a Node),
    Refused(&'a Refusal),
}

impl Kid<'_> {
    fn seq(&self) -> u64 {
        match self {
            Kid::Node(n) => n.created_seq,
            Kid::Refused(r) => r.seq,
        }
    }
}

fn build_rows(tree: &Tree) -> Vec<Row> {
    let mut kids: HashMap<&str, Vec<Kid>> = HashMap::new();
    for n in &tree.nodes {
        if let Some(p) = &n.parent {
            kids.entry(p.as_str()).or_default().push(Kid::Node(n));
        }
    }
    for r in &tree.refused {
        kids.entry(r.parent.as_str())
            .or_default()
            .push(Kid::Refused(r));
    }
    for list in kids.values_mut() {
        list.sort_by_key(Kid::seq);
    }

    let mut rows = Vec::new();
    let Some(root) = tree.node(&tree.root) else {
        return rows;
    };
    rows.push(node_row(tree, root, 0, String::new()));
    walk(tree, &kids, &root.hash, String::new(), 1, &mut rows);
    rows
}

fn walk(
    tree: &Tree,
    kids: &HashMap<&str, Vec<Kid>>,
    parent: &str,
    cont: String,
    depth: usize,
    rows: &mut Vec<Row>,
) {
    let Some(list) = kids.get(parent) else {
        return;
    };
    let n = list.len();
    for (i, kid) in list.iter().enumerate() {
        let last = i + 1 == n;
        let prefix = format!(
            "{cont}{}",
            if last {
                "\u{2514}\u{2500} "
            } else {
                "\u{251c}\u{2500} "
            }
        );
        match kid {
            Kid::Node(node) => {
                rows.push(node_row(tree, node, depth, prefix));
                let next = format!("{cont}{}", if last { "   " } else { "\u{2502}  " });
                walk(tree, kids, &node.hash, next, depth + 1, rows);
            }
            Kid::Refused(r) => rows.push(Row {
                id: RowId::Refused(r.seq),
                seq: r.seq,
                depth,
                prefix,
                short: String::new(),
                status: RowStatus::Refused,
                is_current: false,
                title: r
                    .hypothesis
                    .clone()
                    .unwrap_or_else(|| "(no hypothesis)".to_string()),
                gain_pp: None,
                median: None,
            }),
        }
    }
}

fn node_row(tree: &Tree, node: &Node, depth: usize, prefix: String) -> Row {
    let is_root = node.parent.is_none();
    let edge = tree.edge_for_child(&node.hash);
    let median = node_median_of(node);
    Row {
        id: RowId::Version(node.hash.clone()),
        seq: node.created_seq,
        depth,
        prefix,
        short: short7(&node.hash),
        status: verdict_status(node.verdict, is_root),
        is_current: node.hash == tree.current,
        title: node
            .hypothesis
            .clone()
            .unwrap_or_else(|| "starting rules".to_string()),
        gain_pp: edge.and_then(|e| gain(e.parent_median, e.child_median)),
        median,
    }
}

fn node_median_of(node: &Node) -> Option<f64> {
    node_median(node)
}

fn build_progress(tree: &Tree, events: &[Event], cfg: &ConfigView) -> Option<Progress> {
    // The newest event that points at a version decides what is being benchmarked.
    let target = events
        .iter()
        .rev()
        .find_map(|e| match Payload::from_event(e) {
            Ok(Payload::RunRecorded(p)) => Some(p.node),
            Ok(Payload::MutationProposed(p)) => Some(p.child),
            Ok(Payload::Verdict(p)) => Some(p.child),
            _ => None,
        })?;
    let node = tree.node(&target)?;
    let mut runs: Vec<_> = node.runs.iter().collect();
    runs.sort_by_key(|r| r.run_index);
    let newest_child = tree.edges.last().map(|e| e.child.as_str());
    let role = if node.parent.is_none() {
        Role::Root
    } else if newest_child == Some(node.hash.as_str()) {
        Role::Child
    } else {
        Role::Parent
    };
    Some(Progress {
        version: short7(&node.hash),
        role,
        hypothesis: node.hypothesis.clone(),
        runs_done: runs.len() as u32,
        repeats: cfg.repeats,
        rates: runs
            .iter()
            .map(|r| {
                if r.total == 0 {
                    0.0
                } else {
                    f64::from(r.passed) / f64::from(r.total)
                }
            })
            .collect(),
        settled: node.verdict.is_some(),
    })
}

fn build_feed(tree: &Tree) -> Vec<FeedItem> {
    let mut items: Vec<FeedItem> = Vec::new();
    for e in &tree.edges {
        if let (Some(v), Some(seq)) = (e.verdict, e.verdict_seq) {
            items.push(FeedItem {
                seq,
                kind: FeedKind::Verdict(v),
                target: RowId::Version(e.child.clone()),
                short: short7(&e.child),
                hypothesis: e.hypothesis.clone(),
                gain_pp: gain(e.parent_median, e.child_median),
                paired_wins: e.paired_wins,
                repeats: e.repeats,
                reason: e.note.clone().unwrap_or_default(),
            });
        }
    }
    for r in &tree.refused {
        items.push(FeedItem {
            seq: r.seq,
            kind: FeedKind::Refused,
            target: RowId::Refused(r.seq),
            short: short7(&r.parent),
            hypothesis: r
                .hypothesis
                .clone()
                .unwrap_or_else(|| "(no hypothesis)".to_string()),
            gain_pp: None,
            paired_wins: None,
            repeats: None,
            reason: r.reason.clone(),
        });
    }
    items.sort_by_key(|i| std::cmp::Reverse(i.seq));
    items
}

fn describe_last(events: &[Event], cfg: &ConfigView) -> Option<Activity> {
    let e = events.last()?;
    let text = match Payload::from_event(e).ok()? {
        Payload::Init(_) => "log started".to_string(),
        Payload::MutationProposed(p) => {
            format!("proposed {}: {}", short7(&p.child), p.hypothesis)
        }
        Payload::MutationRefused(p) => format!(
            "refused a mutation: {}",
            p.hypothesis.unwrap_or_else(|| p.reason.clone())
        ),
        Payload::RunRecorded(p) => format!(
            "run {} of {} recorded for {}: {} of {} tasks passed",
            p.run_index + 1,
            cfg.repeats,
            short7(&p.node),
            p.passed,
            p.total
        ),
        Payload::Verdict(p) => format!(
            "verdict for {}: {}",
            short7(&p.child),
            p.verdict.as_str().to_uppercase()
        ),
        Payload::Applied(p) => format!(
            "applied {} to {}{}",
            short7(&p.version),
            p.rules_file,
            if p.forced { " (forced)" } else { "" }
        ),
    };
    Some(Activity { seq: e.seq, text })
}

impl WatchState {
    /// A state with no log content at all.
    pub fn empty(chain: ChainStatus, config: ConfigView) -> WatchState {
        WatchState {
            chain,
            config,
            rules_file: String::new(),
            model: String::new(),
            tree: None,
            rows: Vec::new(),
            progress: None,
            feed: Vec::new(),
            activity: None,
            tokens_total: 0,
            tokens_max_run: 0,
            accepted: 0,
            rejected: 0,
            noise: 0,
            overfit: 0,
            pending: 0,
            refused: 0,
        }
    }

    /// Build the view model from chain checked events. When `chain` is not
    /// valid, `events` are the ones before the first problem, so the view shows
    /// the last good state next to a loud warning.
    pub fn build(events: &[Event], mut chain: ChainStatus, config: ConfigView) -> WatchState {
        let mut state = WatchState::empty(chain.clone(), config);
        if events.is_empty() {
            return state;
        }
        let tree = match Tree::from_events(events) {
            Ok(t) => t,
            Err(e) => {
                chain.valid = false;
                chain.problems.push(format!("lineage: {e}"));
                state.chain = chain;
                return state;
            }
        };
        let summary = tree.summary();
        state.rules_file = tree.rules_file.clone();
        state.model = tree.model.clone();
        state.rows = build_rows(&tree);
        state.progress = build_progress(&tree, events, &config);
        state.feed = build_feed(&tree);
        state.activity = describe_last(events, &config);
        state.tokens_total = summary.tokens_used;
        state.tokens_max_run = tree
            .nodes
            .iter()
            .flat_map(|n| n.runs.iter().map(|r| r.tokens_used))
            .max()
            .unwrap_or(0);
        state.accepted = summary.accepted;
        state.rejected = summary.rejected;
        state.noise = summary.noise;
        state.overfit = summary.overfit;
        state.pending = summary.pending;
        state.refused = summary.refused;
        state.tree = Some(tree);
        state
    }

    pub fn row(&self, id: &RowId) -> Option<&Row> {
        self.rows.iter().find(|r| &r.id == id)
    }

    pub fn row_index(&self, id: &RowId) -> Option<usize> {
        self.rows.iter().position(|r| &r.id == id)
    }

    /// The row created by the newest event.
    pub fn newest_row(&self) -> Option<&Row> {
        self.rows.iter().max_by_key(|r| r.seq)
    }

    pub fn current_row(&self) -> Option<&Row> {
        self.rows.iter().find(|r| r.is_current)
    }

    /// True when the log has an init event and no attempts yet.
    pub fn is_fresh(&self) -> bool {
        self.tree
            .as_ref()
            .is_some_and(|t| t.edges.is_empty() && t.refused.is_empty())
    }

    pub fn details(&self, id: &RowId) -> Option<Details> {
        let tree = self.tree.as_ref()?;
        let row = self.row(id)?;
        match id {
            RowId::Refused(seq) => {
                let r = tree.refused.iter().find(|r| r.seq == *seq)?;
                let mut fields = vec![
                    ("tried on".to_string(), short7(&r.parent)),
                    ("log entry".to_string(), r.seq.to_string()),
                ];
                if let Some(p) = &r.provider {
                    fields.push(("provider".into(), p.clone()));
                }
                if let Some(s) = r.seed {
                    fields.push(("seed".into(), s.to_string()));
                }
                Some(Details {
                    title: "Refused mutation".to_string(),
                    status: RowStatus::Refused,
                    is_current: false,
                    fields,
                    hypothesis: r.hypothesis.clone(),
                    reason: Some(r.reason.clone()),
                    runs: Vec::new(),
                    hash: None,
                    parent_hash: None,
                })
            }
            RowId::Version(hash) => {
                let node = tree.node(hash)?;
                let edge = tree.edge_for_child(hash);
                let mut fields = vec![("generation".to_string(), node.generation.to_string())];
                if let Some(m) = row.median {
                    fields.push(("median pass rate".into(), pct(m)));
                }
                let mut reason = None;
                if let Some(e) = edge {
                    fields.push(("parent".into(), short7(&e.parent)));
                    if let (Some(p), Some(c)) = (e.parent_median, e.child_median) {
                        fields.push((
                            "parent to child".into(),
                            format!("{} to {}", pct(p), pct(c)),
                        ));
                    }
                    if let Some(g) = gain(e.parent_median, e.child_median) {
                        fields.push(("gain".into(), points(g)));
                    }
                    if let (Some(w), Some(n)) = (e.paired_wins, e.repeats) {
                        fields.push((
                            "paired wins".into(),
                            format!("{w} of {n} (needs {})", self.config.min_paired_wins),
                        ));
                    }
                    reason = e.note.clone().or_else(|| {
                        e.verdict
                            .is_none()
                            .then(|| "No verdict yet. Waiting for the runs and verify.".to_string())
                    });
                }
                let mut runs: Vec<_> = node.runs.iter().collect();
                runs.sort_by_key(|r| r.run_index);
                let run_lines = runs
                    .iter()
                    .map(|r| {
                        format!(
                            "run {}: {} of {} tasks, {} tokens{}",
                            r.run_index + 1,
                            r.passed,
                            r.total,
                            r.tokens_used,
                            if r.budget_exhausted {
                                ", budget used up"
                            } else {
                                ""
                            }
                        )
                    })
                    .collect();
                Some(Details {
                    title: format!("Version {}", short7(hash)),
                    status: row.status,
                    is_current: row.is_current,
                    fields,
                    hypothesis: node.hypothesis.clone(),
                    reason,
                    runs: run_lines,
                    hash: Some(hash.clone()),
                    parent_hash: node.parent.clone(),
                })
            }
        }
    }
}

/// "58%". Rounds to a whole percent.
pub fn pct(rate: f64) -> String {
    format!("{:.0}%", rate * 100.0)
}

/// "+33 pts", "-42 pts", "0 pts". Rounds to whole points.
pub fn points(gain: f64) -> String {
    let r = gain.round();
    if r == 0.0 {
        "0 pts".to_string()
    } else {
        format!("{}{} pts", if r > 0.0 { "+" } else { "-" }, r.abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    #[test]
    fn empty_log_has_no_rows() {
        let s = WatchState::build(&[], ChainStatus::ok(0), ConfigView::default());
        assert!(s.rows.is_empty() && s.tree.is_none() && s.progress.is_none());
    }

    #[test]
    fn fresh_log_shows_only_the_root() {
        let s = build(&[init()]);
        assert!(s.is_fresh());
        assert_eq!(s.rows.len(), 1);
        assert_eq!(s.rows[0].status, RowStatus::Root);
        assert!(s.rows[0].is_current);
        assert!(s.progress.is_none());
        assert!(s.feed.is_empty());
        assert_eq!(s.activity.as_ref().unwrap().text, "log started");
    }

    #[test]
    fn rows_are_indented_and_ordered_by_creation() {
        let s = build(&story());
        let ids: Vec<String> = s
            .rows
            .iter()
            .map(|r| match &r.id {
                RowId::Version(h) => h[..1].to_string(),
                RowId::Refused(q) => format!("r{q}"),
            })
            .collect();
        // a root; b under a (accepted); c, d, refused, e under b.
        assert_eq!(ids[0], "a");
        assert_eq!(ids[1], "b");
        assert_eq!(s.rows[0].depth, 0);
        assert_eq!(s.rows[1].depth, 1);
        assert!(s.rows[2..].iter().all(|r| r.depth == 2));
        assert_eq!(s.rows.len(), 8);
        // Siblings keep the order they were created in.
        let seqs: Vec<u64> = s.rows[2..].iter().map(|r| r.seq).collect();
        let mut sorted = seqs.clone();
        sorted.sort();
        assert_eq!(seqs, sorted);
    }

    #[test]
    fn tree_guides_close_the_last_sibling() {
        let s = build(&story());
        assert_eq!(s.rows[0].prefix, "");
        assert_eq!(s.rows[1].prefix, "\u{2514}\u{2500} ");
        assert_eq!(s.rows[2].prefix, "   \u{251c}\u{2500} ");
        assert_eq!(s.rows.last().unwrap().prefix, "   \u{2514}\u{2500} ");
    }

    #[test]
    fn overfit_is_its_own_status_and_does_not_move_current() {
        let s = build(&[
            init(),
            proposed('a', 'b', "narrow"),
            verdict('a', 'b', VerdictKind::Overfit, 0.5, 0.75, 5),
        ]);
        let b = s
            .rows
            .iter()
            .find(|r| matches!(&r.id, RowId::Version(h) if h.starts_with('b')))
            .unwrap();
        assert_eq!(b.status, RowStatus::Overfit);
        assert_eq!(b.status.label(), "overfit");
        assert!(!b.is_current);
        assert_eq!((s.accepted, s.overfit), (0, 1));
    }

    #[test]
    fn statuses_and_current_follow_the_log() {
        let s = build(&story());
        let status = |c: char| {
            s.rows
                .iter()
                .find(|r| matches!(&r.id, RowId::Version(h) if h.starts_with(c)))
                .unwrap()
                .status
        };
        assert_eq!(status('a'), RowStatus::Root);
        assert_eq!(status('b'), RowStatus::Accepted);
        assert_eq!(status('c'), RowStatus::Rejected);
        assert_eq!(status('d'), RowStatus::Noise);
        assert_eq!(status('e'), RowStatus::Pending);
        let current: Vec<&Row> = s.rows.iter().filter(|r| r.is_current).collect();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].short, "bbbbbbb");
        assert_eq!(s.refused, 3);
        assert_eq!((s.accepted, s.rejected, s.noise, s.pending), (1, 1, 1, 1));
        assert_eq!(
            s.rows
                .iter()
                .filter(|r| r.status == RowStatus::Refused)
                .count(),
            3
        );
    }

    #[test]
    fn gains_and_medians_come_from_the_verdicts_and_runs() {
        let s = build(&story());
        let b = s.rows.iter().find(|r| r.short == "bbbbbbb").unwrap();
        assert!((b.gain_pp.unwrap() - 50.0).abs() < 1e-9);
        assert_eq!(b.median, Some(0.8));
        let root = &s.rows[0];
        assert_eq!(root.median, Some(0.3));
        assert_eq!(root.gain_pp, None);
    }

    #[test]
    fn progress_follows_the_newest_run() {
        let events = vec![
            init(),
            run('a', 0, 3),
            proposed('a', 'b', "x"),
            run('b', 0, 4),
            run('b', 1, 6),
        ];
        let s = build(&events);
        let p = s.progress.as_ref().unwrap();
        assert_eq!(p.version, "bbbbbbb");
        assert_eq!(p.role, Role::Child);
        assert_eq!((p.runs_done, p.repeats), (2, 5));
        assert_eq!(p.rates, vec![0.4, 0.6]);
        assert!(!p.settled);
        assert!((p.ratio() - 0.4).abs() < 1e-9);
        assert!(s
            .activity
            .as_ref()
            .unwrap()
            .text
            .starts_with("run 2 of 5 recorded for bbbbbbb"));
    }

    #[test]
    fn progress_with_no_runs_yet_points_at_the_new_child() {
        let s = build(&[init(), proposed('a', 'b', "x")]);
        let p = s.progress.as_ref().unwrap();
        assert_eq!((p.version.as_str(), p.runs_done), ("bbbbbbb", 0));
        assert_eq!(p.ratio(), 0.0);
    }

    #[test]
    fn progress_of_the_parent_run_is_labelled_parent() {
        let events = vec![
            init(),
            run('a', 0, 3),
            proposed('a', 'b', "x"),
            proposed('b', 'c', "y"),
            run('b', 0, 4),
        ];
        let s = build(&events);
        assert_eq!(s.progress.as_ref().unwrap().role, Role::Parent);
    }

    #[test]
    fn a_verdict_settles_progress() {
        let mut events = vec![
            init(),
            run('a', 0, 3),
            proposed('a', 'b', "x"),
            run('b', 0, 9),
        ];
        events.push(verdict('a', 'b', VerdictKind::Accepted, 0.3, 0.9, 5));
        let s = build(&events);
        assert!(s.progress.as_ref().unwrap().settled);
        assert!(s.activity.as_ref().unwrap().text.contains("ACCEPTED"));
    }

    #[test]
    fn feed_is_newest_first_and_includes_refusals() {
        let s = build(&story());
        assert_eq!(s.feed.len(), 6);
        let seqs: Vec<u64> = s.feed.iter().map(|f| f.seq).collect();
        let mut sorted = seqs.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(seqs, sorted);
        assert!(s.feed.iter().any(|f| f.kind == FeedKind::Refused));
        let acc = s
            .feed
            .iter()
            .find(|f| f.kind == FeedKind::Verdict(VerdictKind::Accepted))
            .unwrap();
        assert_eq!(acc.paired_wins, Some(5));
        assert_eq!(acc.hypothesis, "Run the test suite before finishing.");
        assert!((acc.gain_pp.unwrap() - 50.0).abs() < 1e-9);
        // A pending child has no feed entry.
        assert!(!s
            .feed
            .iter()
            .any(|f| f.short == "eeeeeee" && matches!(f.kind, FeedKind::Verdict(_))));
    }

    #[test]
    fn totals_sum_tokens_and_find_the_biggest_run() {
        let s = build(&story());
        assert_eq!(s.tokens_total, 20);
        assert_eq!(s.tokens_max_run, 10);
    }

    #[test]
    fn details_for_a_judged_version() {
        let s = build(&story());
        let d = s.details(&RowId::Version(h('b'))).expect("details");
        assert_eq!(d.status, RowStatus::Accepted);
        assert!(d.is_current);
        assert_eq!(
            d.hypothesis.as_deref(),
            Some("Run the test suite before finishing.")
        );
        assert_eq!(d.reason.as_deref(), Some("because accepted"));
        let field = |k: &str| {
            d.fields
                .iter()
                .find(|(a, _)| a == k)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(field("gain").as_deref(), Some("+50 pts"));
        assert_eq!(field("paired wins").as_deref(), Some("5 of 5 (needs 4)"));
        assert_eq!(field("parent to child").as_deref(), Some("30% to 80%"));
        assert_eq!(d.runs.len(), 1);
        assert_eq!(d.parent_hash, Some(h('a')));
    }

    #[test]
    fn details_for_pending_and_refused() {
        let s = build(&story());
        let pending = s.details(&RowId::Version(h('e'))).unwrap();
        assert!(pending.reason.unwrap().contains("No verdict yet"));
        let refused_row = s
            .rows
            .iter()
            .find(|r| r.status == RowStatus::Refused)
            .unwrap();
        let d = s.details(&refused_row.id).unwrap();
        assert_eq!(d.status, RowStatus::Refused);
        assert!(d.reason.is_some());
        assert!(d.hash.is_none());
        assert!(s.details(&RowId::Refused(9999)).is_none());
    }

    #[test]
    fn newest_row_is_the_one_with_the_highest_seq() {
        let s = build(&story());
        let newest = s.newest_row().unwrap();
        assert_eq!(newest.seq, s.rows.iter().map(|r| r.seq).max().unwrap());
    }

    #[test]
    fn a_lineage_that_cannot_be_rebuilt_marks_the_chain_bad() {
        // Verdict for a child that was never proposed.
        let s = build(&[
            init(),
            verdict('a', 'z', VerdictKind::Accepted, 0.1, 0.9, 5),
        ]);
        assert!(!s.chain.valid);
        assert!(s.chain.problems[0].starts_with("lineage:"));
        assert!(s.rows.is_empty());
    }

    #[test]
    fn building_twice_gives_the_same_state() {
        let a = build(&story());
        let b = build(&story());
        assert_eq!(a.rows, b.rows);
        assert_eq!(a.feed, b.feed);
        assert_eq!(a.progress, b.progress);
    }

    #[test]
    fn number_formats() {
        assert_eq!(pct(0.9167), "92%");
        assert_eq!(points(33.3), "+33 pts");
        assert_eq!(points(-41.7), "-42 pts");
        assert_eq!(points(-0.2), "0 pts");
    }
}
