//! Model driven mutation (`mutate`) and the generation loop (`evolve`).
//!
//! `mutate` asks a model for one small rules change plus a hypothesis, checks
//! the answer, and writes `mutation_proposed` or `mutation_refused`. `evolve`
//! repeats mutate, run, verify for a number of generations and only moves
//! `current` forward when the fixed rule says accepted.

use crate::bench::{
    record_mutation, run_version, verify_child, MutationOrigin, ProposeOutcome, RunOptions,
    VerifyOutcome,
};
use crate::error::{Error, Result};
use crate::event::{MutationRefusedPayload, Payload, VerdictKind};
use crate::hash::sha256_hex;
use crate::model::{Model, MutationRequest};
use crate::repo::Repo;
use crate::tree::Tree;
use serde::Serialize;

/// Most history lines put in a prompt. Older ones are dropped.
const MAX_HISTORY_LINES: usize = 40;

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(12)]
}

/// A plain text account of what has been tried: accepted, rejected, noise,
/// pending and refused changes, oldest first. Same tree, same text.
pub fn history_text(tree: &Tree) -> String {
    let mut lines: Vec<(u64, String)> = Vec::new();
    for e in &tree.edges {
        let label = e.verdict.map(|v| v.as_str()).unwrap_or("pending");
        let detail = match (e.parent_median, e.child_median, e.paired_wins, e.repeats) {
            (Some(p), Some(c), Some(w), Some(r)) => format!(
                " (median {:.0}% to {:.0}%, {w} of {r} paired wins)",
                p * 100.0,
                c * 100.0
            ),
            _ => String::new(),
        };
        lines.push((e.seq, format!("- [{label}] {}{detail}", e.hypothesis)));
    }
    for r in &tree.refused {
        let hyp = r.hypothesis.as_deref().unwrap_or("(no hypothesis)");
        lines.push((r.seq, format!("- [refused] {hyp}: {}", r.reason)));
    }
    lines.sort_by_key(|(seq, _)| *seq);
    if lines.is_empty() {
        return "No attempts yet.".to_string();
    }
    let skip = lines.len().saturating_sub(MAX_HISTORY_LINES);
    let mut out = String::new();
    if skip > 0 {
        out.push_str(&format!("({skip} older attempts not shown)\n"));
    }
    let body: Vec<String> = lines.into_iter().skip(skip).map(|(_, l)| l).collect();
    out.push_str(&body.join("\n"));
    out
}

/// The prompt sent for a mutation: the current rules, the history, and the
/// size limit.
pub fn build_mutation_prompt(
    parent_rules: &str,
    parent_hash: &str,
    tree: &Tree,
    max_change_bytes: usize,
) -> String {
    format!(
        "You are improving the agent rules file of a software project. The AI model that follows \
these rules stays the same. Only the rules text changes.\n\
\n\
Propose ONE small change to the rules below, and say in one or two plain sentences why it should \
help (your hypothesis).\n\
\n\
Constraints:\n\
- Change at most {max_change_bytes} bytes in total (text removed plus text added counts as the larger of the two).\n\
- Do not repeat a change that appears in the history below.\n\
- Reply with the hypothesis and the complete new rules text.\n\
\n\
## Current rules (version {})\n\
{parent_rules}\n\
## History of this lineage\n\
{}\n",
        short(parent_hash),
        history_text(tree)
    )
}

/// Seed for the `n`th mutation attempt of a session that started from
/// `base_seed`. Used by `evolve`.
pub fn mutation_seed(base_seed: u64, attempt: usize) -> u64 {
    let d = sha256_hex(format!("rules-evolve/mutate/{base_seed}/{attempt}").as_bytes());
    u64::from_str_radix(&d[..12], 16).expect("hex")
}

#[derive(Debug, Clone)]
pub struct MutateOptions {
    /// `root`, `current`, a hash, or a unique prefix.
    pub parent: String,
    pub seed: u64,
}

/// What `mutate` did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MutateOutcome {
    #[serde(flatten)]
    pub result: ProposeOutcome,
    pub provider: String,
    pub seed: u64,
    pub tokens_used: u64,
}

/// Ask the model for a change to `opts.parent` and record the result.
///
/// Errors from the model (including a replay miss) are returned and nothing is
/// written. A model answer that breaks a rule is written as a refusal.
pub fn mutate(repo: &Repo, model: &dyn Model, opts: &MutateOptions) -> Result<MutateOutcome> {
    let config = repo.config()?;
    // A real provider must not run without isolation. Refuse before any call.
    crate::isolation::check_policy(model.kind(), repo.sandbox_settings()?.isolation)?;
    let tree = Tree::from_events(&repo.events()?)?;
    let parent = tree.resolve(&opts.parent)?;
    let parent_bytes = repo.store().get_blob(&parent)?;
    let parent_text = String::from_utf8(parent_bytes)
        .map_err(|_| Error::Input("the parent rules file must be UTF-8 text".into()))?;
    let limit = config.mutation.max_change_bytes;
    let cap = config.budget.max_tokens_per_run;
    let prompt = build_mutation_prompt(&parent_text, &parent, &tree, limit);
    let request = MutationRequest {
        model: &tree.model,
        prompt: &prompt,
        parent_rules: &parent_text,
        parent_hash: &parent,
        seed: opts.seed,
        max_change_bytes: limit,
        max_tokens: cap,
    };
    let provider = model.name();

    let estimate = model.estimate_mutation_tokens(&request);
    if estimate > cap {
        let reason =
            format!("the mutation call would need about {estimate} tokens, over the cap of {cap}");
        return refuse(repo, &parent, &provider, opts.seed, 0, reason, limit);
    }
    let response = model.propose_mutation(&request)?;
    if response.tokens_used > cap {
        let reason = format!(
            "the model reported {} tokens, over the cap of {cap}, so its answer was thrown away",
            response.tokens_used
        );
        return refuse(repo, &parent, &provider, opts.seed, cap, reason, limit);
    }
    let result = record_mutation(
        repo,
        &config,
        &parent,
        response.new_rules.as_bytes(),
        response.hypothesis.trim(),
        &MutationOrigin {
            seed: Some(opts.seed),
            provider: Some(provider.clone()),
            tokens_used: Some(response.tokens_used),
        },
    )?;
    Ok(MutateOutcome {
        result,
        provider,
        seed: opts.seed,
        tokens_used: response.tokens_used,
    })
}

fn refuse(
    repo: &Repo,
    parent: &str,
    provider: &str,
    seed: u64,
    tokens: u64,
    reason: String,
    limit: usize,
) -> Result<MutateOutcome> {
    let event = repo.append(&Payload::MutationRefused(MutationRefusedPayload {
        parent: parent.to_string(),
        hypothesis: None,
        reason: reason.clone(),
        seed: Some(seed),
        provider: Some(provider.to_string()),
        tokens_used: Some(tokens),
    }))?;
    Ok(MutateOutcome {
        result: ProposeOutcome {
            outcome: "refused".into(),
            parent: parent.to_string(),
            child: None,
            hypothesis: String::new(),
            change_bytes: 0,
            max_change_bytes: limit,
            reason: Some(reason),
            event_seq: event.seq,
        },
        provider: provider.to_string(),
        seed,
        tokens_used: tokens,
    })
}

#[derive(Debug, Clone)]
pub struct EvolveOptions {
    pub generations: u32,
    /// Base seed for runs (shared by parent and child) and for mutations.
    pub base_seed: u64,
}

/// One generation of `evolve`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Generation {
    pub generation: u32,
    /// The version this generation tried to improve.
    pub parent: String,
    pub mutation: MutateOutcome,
    /// Runs written for the parent and the child in this generation.
    pub parent_runs_recorded: usize,
    pub child_runs_recorded: usize,
    /// `None` when the mutation was refused.
    pub verdict: Option<VerifyOutcome>,
    /// `current` after this generation.
    pub current: String,
    pub tokens_used: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EvolveReport {
    pub provider: String,
    pub base_seed: u64,
    pub generations_requested: u32,
    pub start: String,
    pub end: String,
    pub accepted: u32,
    pub rejected: u32,
    pub noise: u32,
    pub refused: u32,
    pub total_tokens: u64,
    /// Isolation backend the checks ran with. `None` when no check ran (every
    /// attempt was refused).
    pub isolation: Option<String>,
    pub generations: Vec<Generation>,
}

/// Loop `generations` times: mutate from `current`, run parent and child, and
/// verify. `current` moves only when a child is accepted.
pub fn evolve(repo: &Repo, model: &dyn Model, opts: &EvolveOptions) -> Result<EvolveReport> {
    let first = Tree::from_events(&repo.events()?)?;
    let mut report = EvolveReport {
        provider: model.name(),
        base_seed: opts.base_seed,
        generations_requested: opts.generations,
        start: first.current.clone(),
        end: first.current.clone(),
        accepted: 0,
        rejected: 0,
        noise: 0,
        refused: 0,
        total_tokens: 0,
        isolation: None,
        generations: Vec::new(),
    };

    for g in 0..opts.generations {
        let tree = Tree::from_events(&repo.events()?)?;
        let parent = tree.current.clone();
        let attempt = tree.edges.len() + tree.refused.len();
        let mutation = mutate(
            repo,
            model,
            &MutateOptions {
                parent: parent.clone(),
                seed: mutation_seed(opts.base_seed, attempt),
            },
        )?;
        let mut tokens = mutation.tokens_used;
        let mut gen = Generation {
            generation: g + 1,
            parent: parent.clone(),
            parent_runs_recorded: 0,
            child_runs_recorded: 0,
            verdict: None,
            current: parent.clone(),
            tokens_used: 0,
            mutation,
        };
        match gen.mutation.result.child.clone() {
            None => report.refused += 1,
            Some(child) => {
                let run_opts = |version: &str| RunOptions {
                    version: version.to_string(),
                    repeats: None,
                    base_seed: opts.base_seed,
                };
                let parent_runs = run_version(repo, model, &run_opts(&parent))?;
                let child_runs = run_version(repo, model, &run_opts(&child))?;
                report.isolation.get_or_insert(child_runs.isolation.clone());
                gen.parent_runs_recorded = parent_runs.runs.len();
                gen.child_runs_recorded = child_runs.runs.len();
                tokens += parent_runs.total_tokens + child_runs.total_tokens;
                let verdict = verify_child(repo, &child)?;
                match verdict.decision.verdict {
                    VerdictKind::Accepted => report.accepted += 1,
                    VerdictKind::Rejected => report.rejected += 1,
                    VerdictKind::Noise => report.noise += 1,
                }
                gen.verdict = Some(verdict);
            }
        }
        gen.current = Tree::from_events(&repo.events()?)?.current;
        gen.tokens_used = tokens;
        report.total_tokens += tokens;
        report.end = gen.current.clone();
        report.generations.push(gen);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::{init, InitOptions};
    use crate::model::{MockConfig, MockModel};
    use tempfile::{tempdir, TempDir};

    const N: usize = 6;

    /// Mock with five candidate changes. Markers decide how they score.
    pub(crate) const MOCK: &str = "seed = 2\nbase_pass_probability = 0.3\ntokens_per_call = 100\n\
        [[marker]]\ntext = \"GOOD\"\ndelta = 0.6\n\
        [[marker]]\ntext = \"BAD\"\ndelta = -0.3\n\
        [[mutation]]\nhypothesis = \"add the good hint\"\nappend = \"GOOD\"\n\
        [[mutation]]\nhypothesis = \"add the bad hint\"\nappend = \"BAD\"\n\
        [[mutation]]\nhypothesis = \"\"\nappend = \"empty hypothesis\"\n\
        [[mutation]]\nhypothesis = \"does nothing\"\n\
        [[mutation]]\nhypothesis = \"rewrite everything\"\nappend = \"@@BIG@@\"\n";

    fn fixture(mock: &str) -> (TempDir, Repo) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("AGENTS.md"), "base rules\n").unwrap();
        std::fs::write(root.join("check.sh"), "grep -qx ok \"answers/$1.txt\"\n").unwrap();
        init(&InitOptions {
            dir: root.to_path_buf(),
            rules_file: None,
            model: "mock-model".into(),
        })
        .unwrap();
        let mut tasks = String::from("[sandbox]\nhidden = [\"check.sh\"]\n");
        let mut mock = mock.replace("@@BIG@@", &"x".repeat(1500));
        for i in 0..N {
            tasks.push_str(&format!(
                "[[task]]\nid = \"t{i}\"\nprompt = \"p{i}\"\ncheck = \"sh check.sh t{i}\"\n"
            ));
            mock.push_str(&format!(
                "[[answer]]\ntask = \"t{i}\"\npath = \"answers/t{i}.txt\"\npass_content = \"ok\\n\"\nfail_content = \"no\\n\"\n"
            ));
        }
        std::fs::write(root.join(".rules-evolve/tasks.toml"), tasks).unwrap();
        std::fs::write(root.join(".rules-evolve/mock.toml"), &mock).unwrap();
        let repo = Repo::open(root).unwrap();
        (dir, repo)
    }

    fn mock_model(repo: &Repo) -> MockModel {
        MockModel::new(MockConfig::load(&repo.store().root().join("mock.toml")).unwrap())
    }

    /// A model that always returns the same mutation, for exact checks.
    struct Fixed {
        rules: String,
        hypothesis: String,
        tokens: u64,
    }
    impl Model for Fixed {
        fn name(&self) -> String {
            "fixed".into()
        }

        fn kind(&self) -> crate::model::ProviderKind {
            crate::model::ProviderKind::Local
        }
        fn estimate_tokens(&self, _r: &crate::model::ModelRequest) -> u64 {
            1
        }
        fn complete(&self, _r: &crate::model::ModelRequest) -> Result<crate::model::ModelResponse> {
            unreachable!()
        }
        fn estimate_mutation_tokens(&self, _r: &MutationRequest) -> u64 {
            1
        }
        fn propose_mutation(&self, _r: &MutationRequest) -> Result<crate::model::MutationResponse> {
            Ok(crate::model::MutationResponse {
                new_rules: self.rules.clone(),
                hypothesis: self.hypothesis.clone(),
                tokens_used: self.tokens,
            })
        }
    }

    fn fixed(rules: &str, hyp: &str) -> Fixed {
        Fixed {
            rules: rules.into(),
            hypothesis: hyp.into(),
            tokens: 5,
        }
    }

    fn opts(seed: u64) -> MutateOptions {
        MutateOptions {
            parent: "current".into(),
            seed,
        }
    }

    #[test]
    fn prompt_has_rules_history_and_limit() {
        let (_d, repo) = fixture(MOCK);
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        let p = build_mutation_prompt("base rules\n", &tree.root, &tree, 1000);
        assert!(p.contains("at most 1000 bytes"));
        assert!(p.contains("base rules"));
        assert!(p.contains("No attempts yet."));
        // Same inputs, same prompt.
        assert_eq!(
            p,
            build_mutation_prompt("base rules\n", &tree.root, &tree, 1000)
        );
    }

    #[test]
    fn history_lists_every_kind_of_attempt() {
        let (_d, repo) = fixture(MOCK);
        let m = fixed("base rules\nGOOD\n", "good one");
        mutate(&repo, &m, &opts(1)).unwrap();
        mutate(&repo, &fixed("base rules\n", "same"), &opts(2)).unwrap();
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        let h = history_text(&tree);
        assert!(h.contains("[pending] good one"), "{h}");
        assert!(
            h.contains("[refused] same: the new text is identical to the parent"),
            "{h}"
        );
        let prompt = build_mutation_prompt("x", &tree.root, &tree, 1000);
        assert!(prompt.contains(&h));
    }

    #[test]
    fn valid_mutation_is_recorded_with_provenance() {
        let (_d, repo) = fixture(MOCK);
        let out = mutate(
            &repo,
            &fixed("base rules\nGOOD\n", "  add a hint  "),
            &opts(7),
        )
        .unwrap();
        assert_eq!(out.result.outcome, "proposed");
        assert_eq!(out.result.hypothesis, "add a hint");
        assert_eq!(
            (out.provider.as_str(), out.seed, out.tokens_used),
            ("fixed", 7, 5)
        );
        let ev = repo.events().unwrap();
        let last = ev.last().unwrap();
        assert_eq!(last.kind, crate::event::EventKind::MutationProposed);
        assert_eq!(last.payload["seed"], 7);
        assert_eq!(last.payload["provider"], "fixed");
        assert_eq!(last.payload["tokens_used"], 5);
        let tree = Tree::from_events(&ev).unwrap();
        assert_eq!(tree.nodes.len(), 2);
        let child = out.result.child.unwrap();
        assert_eq!(
            repo.store().get_blob(&child).unwrap(),
            b"base rules\nGOOD\n"
        );
    }

    #[test]
    fn invalid_mutations_are_refused_and_logged() {
        let (_d, repo) = fixture(MOCK);
        let cases: Vec<(Fixed, &str)> = vec![
            (fixed("base rules\nok\n", "  "), "hypothesis is empty"),
            (
                fixed("base rules\n", "no change"),
                "identical to the parent",
            ),
            (fixed("  \n", "blank"), "empty"),
            (
                fixed(&format!("base rules\n{}", "x".repeat(1001)), "too big"),
                "over the limit of 1000",
            ),
        ];
        for (i, (model, expect)) in cases.iter().enumerate() {
            let out = mutate(&repo, model, &opts(i as u64)).unwrap();
            assert_eq!(out.result.outcome, "refused", "{expect}");
            assert!(out.result.child.is_none());
            let reason = out.result.reason.unwrap();
            assert!(reason.contains(expect), "{reason} vs {expect}");
        }
        // A duplicate of an existing version is refused too.
        mutate(&repo, &fixed("base rules\nA\n", "a"), &opts(10)).unwrap();
        let dup = mutate(&repo, &fixed("base rules\nA\n", "a again"), &opts(11)).unwrap();
        assert!(dup.result.reason.unwrap().contains("already exists"));
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        assert_eq!(tree.nodes.len(), 2);
        assert_eq!(tree.refused.len(), 5);
        let refusal_event = repo
            .events()
            .unwrap()
            .into_iter()
            .find(|e| e.kind == crate::event::EventKind::MutationRefused)
            .unwrap();
        assert_eq!(refusal_event.payload["seed"], 0);
        assert_eq!(refusal_event.payload["provider"], "fixed");
        // The size limit is exact: 1000 bytes changed is fine.
        let ok = mutate(
            &repo,
            &fixed(&format!("base rules\n{}", "y".repeat(1000)), "edge"),
            &opts(12),
        )
        .unwrap();
        assert_eq!(ok.result.outcome, "proposed");
    }

    #[test]
    fn over_budget_mutation_is_refused() {
        let (_d, repo) = fixture(MOCK);
        let mut m = fixed("base rules\nGOOD\n", "x");
        m.tokens = 999_999_999;
        let out = mutate(&repo, &m, &opts(1)).unwrap();
        assert_eq!(out.result.outcome, "refused");
        assert!(out.result.reason.unwrap().contains("over the cap"));
        assert_eq!(out.tokens_used, 200_000);
    }

    #[test]
    fn mock_mutations_are_deterministic_and_seed_dependent() {
        let (_d, repo) = fixture(MOCK);
        let m = mock_model(&repo);
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        let mk = |seed| {
            let req = MutationRequest {
                model: "mock-model",
                prompt: "p",
                parent_rules: "base rules\n",
                parent_hash: &tree.root,
                seed,
                max_change_bytes: 1000,
                max_tokens: 1_000_000,
            };
            m.propose_mutation(&req).unwrap()
        };
        assert_eq!(mk(1), mk(1));
        let picks: std::collections::HashSet<String> = (0..40).map(|s| mk(s).hypothesis).collect();
        assert!(picks.len() >= 4, "{picks:?}");
    }

    #[test]
    fn mock_without_candidates_explains_itself() {
        let (_d, repo) = fixture("seed = 1\n");
        let err = mutate(&repo, &mock_model(&repo), &opts(1)).unwrap_err();
        assert!(err.to_string().contains("[[mutation]]"), "{err}");
        assert_eq!(repo.events().unwrap().len(), 1);
    }

    #[test]
    fn mutate_can_win_and_lose_through_the_whole_loop() {
        let (_d, repo) = fixture(MOCK);
        let good = mutate(&repo, &fixed("base rules\nGOOD\n", "good"), &opts(1)).unwrap();
        let bad = mutate(&repo, &fixed("base rules\nBAD\n", "bad"), &opts(2)).unwrap();
        let m = mock_model(&repo);
        for v in [
            "root".to_string(),
            good.result.child.clone().unwrap(),
            bad.result.child.clone().unwrap(),
        ] {
            run_version(
                &repo,
                &m,
                &RunOptions {
                    version: v,
                    repeats: None,
                    base_seed: 0,
                },
            )
            .unwrap();
        }
        let g = verify_child(&repo, good.result.child.as_deref().unwrap()).unwrap();
        let b = verify_child(&repo, bad.result.child.as_deref().unwrap()).unwrap();
        assert_eq!(g.decision.verdict, VerdictKind::Accepted);
        assert_eq!(b.decision.verdict, VerdictKind::Rejected);
    }

    #[test]
    fn evolve_advances_current_only_on_accepted() {
        let (_d, repo) = fixture(MOCK);
        let m = mock_model(&repo);
        let report = evolve(
            &repo,
            &m,
            &EvolveOptions {
                generations: 8,
                base_seed: 0,
            },
        )
        .unwrap();
        assert_eq!(report.generations.len(), 8);
        assert_eq!(
            report.accepted + report.rejected + report.noise + report.refused,
            8
        );
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        assert_eq!(tree.current, report.end);
        // current only ever changes on an accepted generation.
        let mut cur = report.start.clone();
        for g in &report.generations {
            assert_eq!(g.parent, cur);
            let accepted = g
                .verdict
                .as_ref()
                .map(|v| v.decision.verdict == VerdictKind::Accepted)
                .unwrap_or(false);
            if accepted {
                assert_ne!(g.current, cur);
                assert_eq!(Some(&g.current), g.mutation.result.child.as_ref());
            } else {
                assert_eq!(g.current, cur);
            }
            cur = g.current.clone();
        }
        assert_eq!(cur, report.end);
        // Only a bad run of luck would give all refusals or no children.
        assert!(report.accepted + report.rejected + report.noise > 0);
        assert!(report.total_tokens > 0);
        assert!(repo.verify().unwrap().valid);
    }

    #[test]
    fn evolve_is_deterministic() {
        let go = || {
            let (_d, repo) = fixture(MOCK);
            let m = mock_model(&repo);
            let r = evolve(
                &repo,
                &m,
                &EvolveOptions {
                    generations: 5,
                    base_seed: 3,
                },
            )
            .unwrap();
            let events: Vec<_> = repo
                .events()
                .unwrap()
                .into_iter()
                .map(|e| (e.kind, e.payload))
                .collect();
            (r.end, r.accepted, r.rejected, r.noise, r.refused, events)
        };
        assert_eq!(go(), go());
    }

    #[test]
    fn mutation_seed_is_stable_and_small() {
        assert_eq!(mutation_seed(1, 2), mutation_seed(1, 2));
        assert_ne!(mutation_seed(1, 2), mutation_seed(1, 3));
        assert!(mutation_seed(9, 9) < (1u64 << 48));
    }
}
