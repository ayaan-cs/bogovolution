//! `digest`: turn the lineage into a plain English brief.
//!
//! Everything here is a pure function of the tree. There is no model call and
//! no randomness, so the same tree always gives the same words. The
//! suggestions come from simple rules over the history (pending work, noisy
//! results worth a retest, refusals, word themes in the hypotheses, and a
//! fixed list of common rules file topics that no hypothesis has touched).

use crate::event::VerdictKind;
use crate::export::node_median;
use crate::tree::{Edge, Tree, TreeSummary};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// One judged or pending change.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DigestItem {
    /// Short hash of the version this change made.
    pub version: String,
    pub parent: String,
    pub generation: u32,
    pub hypothesis: String,
    /// `accepted`, `rejected`, `noise` or `pending`.
    pub status: String,
    pub gain_pp: Option<f64>,
    pub parent_median: Option<f64>,
    pub child_median: Option<f64>,
    pub paired_wins: Option<u32>,
    pub repeats: Option<u32>,
    pub reason: Option<String>,
    /// Number of runs recorded for the version.
    pub runs_recorded: usize,
}

/// A refused mutation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DigestRefusal {
    pub hypothesis: Option<String>,
    pub reason: String,
    pub parent: String,
}

/// Where the pass rate went.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Progress {
    /// Median pass rate of the root, if it has runs.
    pub root_pass_rate: Option<f64>,
    /// Median pass rate of the current version, if it has runs.
    pub current_pass_rate: Option<f64>,
    pub gain_pp: Option<f64>,
}

/// Words that came up in hypotheses that worked or that failed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Themes {
    /// Words in accepted hypotheses and in no rejected one.
    pub worked: Vec<String>,
    /// Words in rejected hypotheses and in no accepted one.
    pub failed: Vec<String>,
}

/// A common rules file topic that no hypothesis has mentioned yet.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UntriedArea {
    pub area: String,
    pub idea: String,
}

/// A direction to try next.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suggestion {
    /// `start`, `pending`, `retest`, `smaller`, `build_on`, `themes`,
    /// `avoid`, `variety` or `untried`.
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Digest {
    pub rules_file: String,
    pub model: String,
    pub root: String,
    pub current: String,
    pub counts: TreeSummary,
    pub progress: Progress,
    pub worked: Vec<DigestItem>,
    pub rejected: Vec<DigestItem>,
    pub noise: Vec<DigestItem>,
    pub pending: Vec<DigestItem>,
    pub refused: Vec<DigestRefusal>,
    pub themes: Themes,
    pub untried_areas: Vec<UntriedArea>,
    pub suggestions: Vec<Suggestion>,
}

/// Fixed list of common rules file topics: name, keywords that count as
/// "mentioned", and an example rule to try.
const AREAS: [(&str, &[&str], &str); 11] = [
    (
        "Testing",
        &["test", "tests", "testing", "suite", "coverage"],
        "say when and how to run the tests",
    ),
    (
        "Reading before editing",
        &[
            "read",
            "reading",
            "inspect",
            "explore",
            "understand",
            "context",
        ],
        "ask the agent to read the relevant code before changing it",
    ),
    (
        "Small, focused changes",
        &["small", "minimal", "focused", "scope", "narrow"],
        "keep each change small and limited to what was asked",
    ),
    (
        "Checking its own work",
        &["verify", "review", "recheck", "validate", "confirm"],
        "ask the agent to review its own diff before finishing",
    ),
    (
        "Errors and edge cases",
        &[
            "error",
            "errors",
            "edge",
            "failure",
            "failures",
            "exception",
        ],
        "ask the agent to think about failures and edge cases",
    ),
    (
        "Asking when unsure",
        &[
            "ask",
            "clarify",
            "question",
            "unclear",
            "assumption",
            "assumptions",
        ],
        "ask the agent to state assumptions or ask when a task is unclear",
    ),
    (
        "Code style",
        &[
            "style",
            "convention",
            "conventions",
            "format",
            "lint",
            "naming",
        ],
        "point the agent at the project's style and naming conventions",
    ),
    (
        "Commits and messages",
        &["commit", "commits", "message", "messages", "branch"],
        "say how commits and messages should look",
    ),
    (
        "Documentation",
        &[
            "docs",
            "document",
            "documentation",
            "comment",
            "comments",
            "readme",
        ],
        "say when to update docs and comments",
    ),
    (
        "Tools and commands",
        &[
            "command", "commands", "tool", "tools", "shell", "search", "grep",
        ],
        "list the commands and tools the agent should prefer",
    ),
    (
        "Planning",
        &["plan", "planning", "steps", "outline", "approach"],
        "ask for a short plan before larger changes",
    ),
];

/// Words that say nothing about the topic of a hypothesis.
const STOPWORDS: [&str; 51] = [
    "that", "this", "will", "with", "more", "make", "agent", "agents", "rules", "rule", "before",
    "after", "they", "them", "from", "into", "also", "than", "then", "when", "what", "which",
    "should", "would", "could", "your", "have", "been", "keep", "keeps", "help", "helps", "might",
    "makes", "less", "most", "much", "very", "some", "each", "every", "only", "about", "just",
    "does", "doing", "text", "change", "changes", "little", "being",
];

fn words(text: &str) -> BTreeSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(stem)
        .filter(|w| w.len() >= 4 && !STOPWORDS.contains(&w.as_str()))
        .collect()
}

/// Drop a plural "s" so "test" and "tests" count as one word.
fn stem(w: &str) -> String {
    if w.len() > 4 && w.ends_with('s') && !w.ends_with("ss") {
        w[..w.len() - 1].to_string()
    } else {
        w.to_string()
    }
}

/// Every word, including short and common ones, for matching topic keywords.
fn all_words(text: &str) -> BTreeSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

fn short(hash: &str) -> String {
    hash[..hash.len().min(12)].to_string()
}

fn item(tree: &Tree, e: &Edge) -> DigestItem {
    let node = tree.node(&e.child);
    let parent_median = e
        .parent_median
        .or_else(|| tree.node(&e.parent).and_then(node_median));
    let child_median = e.child_median.or_else(|| node.and_then(node_median));
    DigestItem {
        version: short(&e.child),
        parent: short(&e.parent),
        generation: node.map(|n| n.generation).unwrap_or(0),
        hypothesis: e.hypothesis.clone(),
        status: e
            .verdict
            .map(|v| v.as_str())
            .unwrap_or("pending")
            .to_string(),
        gain_pp: match (parent_median, child_median) {
            (Some(p), Some(c)) => Some((c - p) * 100.0),
            _ => None,
        },
        parent_median,
        child_median,
        paired_wins: e.paired_wins,
        repeats: e.repeats,
        reason: e.note.clone(),
        runs_recorded: node.map(|n| n.runs.len()).unwrap_or(0),
    }
}

/// Top words by count, then alphabetically.
fn top_words(
    counts: &BTreeMap<String, usize>,
    exclude: &BTreeSet<String>,
    n: usize,
) -> Vec<String> {
    let mut v: Vec<(&String, &usize)> = counts
        .iter()
        .filter(|(w, _)| !exclude.contains(*w))
        .collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    v.into_iter().take(n).map(|(w, _)| w.clone()).collect()
}

fn pct(rate: f64) -> String {
    format!("{:.0}%", rate * 100.0)
}

fn quote(text: &str) -> String {
    let t: String = text.trim().to_string();
    t.trim_end_matches('.').to_string()
}

/// Build the digest from a tree.
pub fn build_digest(tree: &Tree) -> Digest {
    let mut worked = Vec::new();
    let mut rejected = Vec::new();
    let mut noise = Vec::new();
    let mut pending = Vec::new();
    for e in &tree.edges {
        let it = item(tree, e);
        match e.verdict {
            Some(VerdictKind::Accepted) => worked.push(it),
            Some(VerdictKind::Rejected) => rejected.push(it),
            Some(VerdictKind::Noise) => noise.push(it),
            None => pending.push(it),
        }
    }
    let refused: Vec<DigestRefusal> = tree
        .refused
        .iter()
        .map(|r| DigestRefusal {
            hypothesis: r.hypothesis.clone(),
            reason: r.reason.clone(),
            parent: short(&r.parent),
        })
        .collect();

    let root_rate = tree.node(&tree.root).and_then(node_median);
    let current_rate = tree.node(&tree.current).and_then(node_median);
    let progress = Progress {
        root_pass_rate: root_rate,
        current_pass_rate: current_rate,
        gain_pp: match (root_rate, current_rate) {
            (Some(r), Some(c)) => Some((c - r) * 100.0),
            _ => None,
        },
    };

    // Themes: words only on one side.
    let mut worked_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut failed_counts: BTreeMap<String, usize> = BTreeMap::new();
    for it in &worked {
        for w in words(&it.hypothesis) {
            *worked_counts.entry(w).or_insert(0) += 1;
        }
    }
    for it in &rejected {
        for w in words(&it.hypothesis) {
            *failed_counts.entry(w).or_insert(0) += 1;
        }
    }
    let worked_set: BTreeSet<String> = worked_counts.keys().cloned().collect();
    let failed_set: BTreeSet<String> = failed_counts.keys().cloned().collect();
    let themes = Themes {
        worked: top_words(&worked_counts, &failed_set, 5),
        failed: top_words(&failed_counts, &worked_set, 5),
    };

    // Untried areas: no hypothesis (of any outcome) mentions the keywords.
    let mut seen = BTreeSet::new();
    for e in &tree.edges {
        seen.extend(all_words(&e.hypothesis));
    }
    for r in &tree.refused {
        if let Some(h) = &r.hypothesis {
            seen.extend(all_words(h));
        }
    }
    let untried_areas: Vec<UntriedArea> = AREAS
        .iter()
        .filter(|(_, keys, _)| !keys.iter().any(|k| seen.contains(*k)))
        .map(|(name, _, idea)| UntriedArea {
            area: (*name).to_string(),
            idea: (*idea).to_string(),
        })
        .collect();

    let suggestions = suggest(
        tree,
        &worked,
        &noise,
        &pending,
        &refused,
        &themes,
        &untried_areas,
    );

    Digest {
        rules_file: tree.rules_file.clone(),
        model: tree.model.clone(),
        root: short(&tree.root),
        current: short(&tree.current),
        counts: tree.summary(),
        progress,
        worked,
        rejected,
        noise,
        pending,
        refused,
        themes,
        untried_areas,
        suggestions,
    }
}

fn suggest(
    tree: &Tree,
    worked: &[DigestItem],
    noise: &[DigestItem],
    pending: &[DigestItem],
    refused: &[DigestRefusal],
    themes: &Themes,
    untried: &[UntriedArea],
) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    let mut add = |kind: &str, text: String| {
        out.push(Suggestion {
            kind: kind.to_string(),
            text,
        })
    };

    if tree.edges.is_empty() && tree.refused.is_empty() {
        add(
            "start",
            "Nothing has been tried yet. Run `mutate` or `evolve --generations N` to start the lineage."
                .to_string(),
        );
    }
    if !pending.is_empty() {
        let list: Vec<String> = pending.iter().map(|p| p.version.clone()).collect();
        add(
            "pending",
            format!(
                "Run and verify the {} pending change{}: {}.",
                pending.len(),
                if pending.len() == 1 { "" } else { "s" },
                list.join(", ")
            ),
        );
    }
    for n in noise.iter().rev().take(3) {
        if n.gain_pp.unwrap_or(0.0) > 0.0 {
            add(
                "retest",
                format!(
                    "Retest \"{}\" with more repeats. It gained {:.1} points but won {} of {} pairs, which was not clear enough.",
                    quote(&n.hypothesis),
                    n.gain_pp.unwrap_or(0.0),
                    n.paired_wins.unwrap_or(0),
                    n.repeats.unwrap_or(0)
                ),
            );
        }
    }
    for r in refused
        .iter()
        .rev()
        .filter(|r| r.reason.contains("over the limit"))
        .take(3)
    {
        {
            if let Some(h) = &r.hypothesis {
                add(
                    "smaller",
                    format!(
                        "Try a smaller version of \"{}\". It was refused for size ({}).",
                        quote(h),
                        r.reason
                    ),
                );
            }
        }
    }
    if let Some(last) = worked.last() {
        add(
            "build_on",
            format!(
                "Build on the latest win, \"{}\" (+{:.1} points). Try a follow up that extends it instead of repeating it.",
                quote(&last.hypothesis),
                last.gain_pp.unwrap_or(0.0)
            ),
        );
    }
    if !themes.worked.is_empty() {
        add(
            "themes",
            format!(
                "Explore more rules around: {}. These words only show up in accepted changes.",
                themes.worked.join(", ")
            ),
        );
    }
    if !themes.failed.is_empty() {
        add(
            "avoid",
            format!(
                "Avoid rules around: {}, unless you change the angle. These words only show up in rejected changes.",
                themes.failed.join(", ")
            ),
        );
    }
    let duplicates = refused
        .iter()
        .filter(|r| r.reason.contains("already exists") || r.reason.contains("identical"))
        .count();
    if duplicates >= 2 {
        add(
            "variety",
            format!(
                "{duplicates} attempts were refused as repeats of text already in the tree. Offer the proposer a wider range of changes."
            ),
        );
    }
    for a in untried.iter().take(5) {
        add(
            "untried",
            format!("Try a rule about {}: {}.", a.area.to_lowercase(), a.idea),
        );
    }
    out
}

fn gain_text(g: Option<f64>) -> String {
    match g {
        Some(v) => format!("{v:+.1} points"),
        None => "no gain recorded".to_string(),
    }
}

fn wins_text(it: &DigestItem) -> String {
    match (it.paired_wins, it.repeats) {
        (Some(w), Some(r)) => format!("{w} of {r} paired wins"),
        _ => "no pairs".to_string(),
    }
}

/// Render the digest as Markdown.
pub fn render_markdown(d: &Digest) -> String {
    let mut s = String::new();
    s.push_str("# Rules lineage digest\n\n");
    s.push_str(&format!(
        "Rules file: `{}`. Model: `{}`.\n\n",
        d.rules_file, d.model
    ));
    let c = &d.counts;
    s.push_str(&format!(
        "{} version{} and {} mutation{} tried: {} accepted, {} rejected, {} noise, {} pending. {} refused before they became versions.\n\n",
        c.nodes,
        if c.nodes == 1 { "" } else { "s" },
        c.edges,
        if c.edges == 1 { "" } else { "s" },
        c.accepted,
        c.rejected,
        c.noise,
        c.pending,
        c.refused
    ));
    s.push_str(&format!(
        "Current version: `{}` (root `{}`). ",
        d.current, d.root
    ));
    match (
        d.progress.root_pass_rate,
        d.progress.current_pass_rate,
        d.progress.gain_pp,
    ) {
        (Some(r), Some(cur), Some(g)) => s.push_str(&format!(
            "Median pass rate moved from {} to {} ({:+.1} points).\n",
            pct(r),
            pct(cur),
            g
        )),
        _ => s.push_str("Not enough runs yet to say how the pass rate has moved.\n"),
    }

    s.push_str("\n## What worked\n\n");
    if d.worked.is_empty() {
        s.push_str("Nothing has been accepted yet.\n");
    }
    for it in &d.worked {
        s.push_str(&format!(
            "- **{}** (`{}`, {}, {}). {}\n",
            quote(&it.hypothesis),
            it.version,
            gain_text(it.gain_pp),
            wins_text(it),
            pct_move(it)
        ));
    }

    s.push_str("\n## What failed\n\n### Rejected\n\n");
    if d.rejected.is_empty() {
        s.push_str("Nothing was rejected.\n");
    }
    for it in &d.rejected {
        s.push_str(&format!(
            "- {} (`{}`, {}). {}\n",
            quote(&it.hypothesis),
            it.version,
            gain_text(it.gain_pp),
            it.reason.clone().unwrap_or_default()
        ));
    }
    s.push_str("\n### Refused\n\n");
    if d.refused.is_empty() {
        s.push_str("Nothing was refused.\n");
    }
    for r in &d.refused {
        s.push_str(&format!(
            "- {}: {}\n",
            r.hypothesis
                .as_deref()
                .map(quote)
                .unwrap_or_else(|| "(no hypothesis)".to_string()),
            r.reason
        ));
    }

    s.push_str("\n## Noise\n\n");
    if d.noise.is_empty() {
        s.push_str("No result was left as noise.\n");
    }
    for it in &d.noise {
        s.push_str(&format!(
            "- {} (`{}`, {}, {}). {}\n",
            quote(&it.hypothesis),
            it.version,
            gain_text(it.gain_pp),
            wins_text(it),
            it.reason.clone().unwrap_or_default()
        ));
    }

    s.push_str("\n## Pending\n\n");
    if d.pending.is_empty() {
        s.push_str("Nothing is waiting for a verdict.\n");
    }
    for it in &d.pending {
        s.push_str(&format!(
            "- {} (`{}`, {} run{} recorded)\n",
            quote(&it.hypothesis),
            it.version,
            it.runs_recorded,
            if it.runs_recorded == 1 { "" } else { "s" }
        ));
    }

    s.push_str("\n## Directions not tried yet\n\n");
    if d.suggestions.is_empty() {
        s.push_str("No suggestions. Every common topic has come up in some hypothesis.\n");
    }
    for g in &d.suggestions {
        s.push_str(&format!("- {}\n", g.text));
    }
    s.push_str(
        "\nThese suggestions come from fixed rules over the history above. No model wrote them.\n",
    );
    s
}

fn pct_move(it: &DigestItem) -> String {
    match (it.parent_median, it.child_median) {
        (Some(p), Some(c)) => format!("Median pass rate {} to {}.", pct(p), pct(c)),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    /// root a -> b accepted, b -> c rejected, b -> d noise, b -> e pending,
    /// plus two refusals.
    fn story() -> Tree {
        tree(&[
            init(),
            run('a', 0, 3),
            proposed('a', 'b', "Run the test suite before finishing."),
            run('b', 0, 8),
            verdict('a', 'b', VerdictKind::Accepted, 0.3, 0.8, 5),
            proposed('b', 'c', "Skip the tests to move faster."),
            verdict('b', 'c', VerdictKind::Rejected, 0.8, 0.4, 0),
            proposed('b', 'd', "Prefer small commits."),
            verdict('b', 'd', VerdictKind::Noise, 0.8, 0.85, 3),
            refused(
                'b',
                Some("Explain everything at length."),
                "change of 1628 bytes is over the limit of 1000",
            ),
            refused(
                'b',
                Some("Prefer small commits."),
                "this content already exists in the tree as abc",
            ),
            refused('b', None, "the hypothesis is empty"),
            proposed('b', 'e', "Read the failing test first."),
            refused(
                'b',
                Some("Prefer small commits."),
                "the new text is identical to the parent",
            ),
        ])
    }

    #[test]
    fn sorts_changes_into_sections() {
        let d = build_digest(&story());
        assert_eq!(d.worked.len(), 1);
        assert_eq!(d.rejected.len(), 1);
        assert_eq!(d.noise.len(), 1);
        assert_eq!(d.pending.len(), 1);
        assert_eq!(d.refused.len(), 4);
        let w = &d.worked[0];
        assert_eq!(w.status, "accepted");
        assert!((w.gain_pp.unwrap() - 50.0).abs() < 1e-9);
        assert_eq!((w.paired_wins, w.repeats), (Some(5), Some(5)));
        assert_eq!(w.version, "bbbbbbbbbbbb");
        assert_eq!(d.rejected[0].reason.as_deref(), Some("because rejected"));
        assert_eq!(d.counts.accepted, 1);
        assert_eq!(d.current, "bbbbbbbbbbbb");
    }

    #[test]
    fn progress_uses_run_medians() {
        let d = build_digest(&story());
        assert_eq!(d.progress.root_pass_rate, Some(0.3));
        assert_eq!(d.progress.current_pass_rate, Some(0.8));
        assert!((d.progress.gain_pp.unwrap() - 50.0).abs() < 1e-9);
        let none = build_digest(&tree(&[init()]));
        assert_eq!(none.progress.gain_pp, None);
    }

    #[test]
    fn themes_only_count_one_sided_words() {
        let d = build_digest(&story());
        // "tests" and "suite" are in the accepted hypothesis. "tests" is also
        // in the rejected one, so it is excluded from both lists.
        assert!(
            d.themes.worked.contains(&"suite".to_string()),
            "{:?}",
            d.themes
        );
        assert!(d.themes.worked.contains(&"finishing".to_string()));
        assert!(!d.themes.worked.contains(&"tests".to_string()));
        assert!(!d.themes.failed.contains(&"tests".to_string()));
        assert!(d.themes.failed.contains(&"faster".to_string()));
        assert!(d.themes.failed.contains(&"skip".to_string()));
    }

    #[test]
    fn untried_areas_skip_topics_already_mentioned() {
        let d = build_digest(&story());
        let names: Vec<&str> = d.untried_areas.iter().map(|a| a.area.as_str()).collect();
        // Mentioned: tests, commits, read.
        assert!(!names.contains(&"Testing"));
        assert!(!names.contains(&"Commits and messages"));
        assert!(!names.contains(&"Reading before editing"));
        assert!(names.contains(&"Documentation"));
        assert!(names.contains(&"Planning"));
        let fresh = build_digest(&tree(&[init()]));
        assert_eq!(fresh.untried_areas.len(), AREAS.len());
    }

    #[test]
    fn suggestions_follow_the_history_in_a_fixed_order() {
        let d = build_digest(&story());
        let kinds: Vec<&str> = d.suggestions.iter().map(|s| s.kind.as_str()).collect();
        assert_eq!(kinds[0], "pending");
        assert_eq!(kinds[1], "retest");
        assert_eq!(kinds[2], "smaller");
        assert_eq!(kinds[3], "build_on");
        assert!(kinds.contains(&"themes"));
        assert!(kinds.contains(&"avoid"));
        assert!(kinds.contains(&"variety"));
        assert_eq!(kinds.iter().filter(|k| **k == "untried").count(), 5);
        let retest = &d.suggestions[1].text;
        assert!(retest.contains("Prefer small commits"), "{retest}");
        assert!(retest.contains("3 of 5 pairs"), "{retest}");
        assert!(d.suggestions[0].text.contains("eeeeeeeeeeee"));
    }

    #[test]
    fn empty_lineage_suggests_starting() {
        let d = build_digest(&tree(&[init()]));
        assert_eq!(d.suggestions[0].kind, "start");
        let md = render_markdown(&d);
        assert!(md.contains("Nothing has been accepted yet."));
        assert!(md.contains("Nothing is waiting for a verdict."));
        assert!(md.contains("Not enough runs yet"));
    }

    #[test]
    fn markdown_is_deterministic_and_has_every_section() {
        let t = story();
        let a = render_markdown(&build_digest(&t));
        let b = render_markdown(&build_digest(&t));
        assert_eq!(a, b);
        for heading in [
            "# Rules lineage digest",
            "## What worked",
            "## What failed",
            "### Rejected",
            "### Refused",
            "## Noise",
            "## Pending",
            "## Directions not tried yet",
        ] {
            assert!(a.contains(heading), "{heading}");
        }
        assert!(a.contains("**Run the test suite before finishing**"));
        assert!(a.contains("+50.0 points"));
        assert!(a.contains("5 of 5 paired wins"));
        assert!(a.contains("Median pass rate moved from 30% to 80% (+50.0 points)."));
        assert!(a.contains(
            "- Explain everything at length: change of 1628 bytes is over the limit of 1000"
        ));
        assert!(a.contains("(no hypothesis): the hypothesis is empty"));
        assert!(a.contains("No model wrote them."));
        assert!(!a.contains('\u{2014}') && !a.contains('\u{2013}'));
    }

    #[test]
    fn golden_markdown_for_a_small_lineage() {
        let t = tree(&[
            init(),
            run('a', 0, 5),
            proposed('a', 'b', "Ask for a plan first."),
            run('b', 0, 9),
            verdict('a', 'b', VerdictKind::Accepted, 0.5, 0.9, 5),
        ]);
        let mut d = build_digest(&t);
        d.suggestions.truncate(2);
        let md = render_markdown(&d);
        let expected = "# Rules lineage digest

Rules file: `AGENTS.md`. Model: `m`.

2 versions and 1 mutation tried: 1 accepted, 0 rejected, 0 noise, 0 pending. 0 refused before they became versions.

Current version: `bbbbbbbbbbbb` (root `aaaaaaaaaaaa`). Median pass rate moved from 50% to 90% (+40.0 points).

## What worked

- **Ask for a plan first** (`bbbbbbbbbbbb`, +40.0 points, 5 of 5 paired wins). Median pass rate 50% to 90%.

## What failed

### Rejected

Nothing was rejected.

### Refused

Nothing was refused.

## Noise

No result was left as noise.

## Pending

Nothing is waiting for a verdict.

## Directions not tried yet

- Build on the latest win, \"Ask for a plan first\" (+40.0 points). Try a follow up that extends it instead of repeating it.
- Explore more rules around: first, plan. These words only show up in accepted changes.

These suggestions come from fixed rules over the history above. No model wrote them.
";
        assert_eq!(md, expected);
    }

    #[test]
    fn json_shape_is_stable() {
        let d = build_digest(&story());
        let v = serde_json::to_value(&d).unwrap();
        for key in [
            "rules_file",
            "model",
            "root",
            "current",
            "counts",
            "progress",
            "worked",
            "rejected",
            "noise",
            "pending",
            "refused",
            "themes",
            "untried_areas",
            "suggestions",
        ] {
            assert!(v.get(key).is_some(), "{key}");
        }
        assert_eq!(v["worked"][0]["status"], "accepted");
    }
}
