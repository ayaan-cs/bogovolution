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
///
/// Held out results never appear here: an `overfit` change is shown as
/// `rejected`, with its train medians only.
pub fn history_text(tree: &Tree) -> String {
    let mut lines: Vec<(u64, String)> = Vec::new();
    for e in &tree.edges {
        let label = match e.verdict {
            Some(VerdictKind::Overfit) => "rejected",
            Some(v) => v.as_str(),
            None => "pending",
        };
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

/// Most bytes of the benchmark results section in a mutation prompt.
pub const MAX_RESULTS_BYTES: usize = 3000;
/// Most failing tasks described in detail in a mutation prompt.
pub const MAX_DETAILED_TASKS: usize = 6;
/// Most bytes of one task prompt quoted in a mutation prompt.
const MAX_TASK_PROMPT_BYTES: usize = 200;

/// How `version` did on its recorded runs: the median pass rate, how often
/// each task passed, and for the tasks that failed most, the task prompt and
/// the failure note kept by the harness (files written and the end of the
/// check output, with hidden file lines already removed). Bounded by
/// [`MAX_RESULTS_BYTES`]. Same tree, same text.
///
/// Train tasks only. Outcomes marked held out, and any task that `tasks`
/// marks held out, are left out entirely (no id, prompt, count or note), and
/// the median is the train median.
pub fn results_text(tree: &Tree, version: &str, tasks: Option<&crate::tasks::TasksFile>) -> String {
    let Some(node) = tree.node(version) else {
        return "No benchmark runs of this version yet.".to_string();
    };
    let mut runs: Vec<&crate::tree::RunSummary> = node.runs.iter().collect();
    if runs.is_empty() {
        return "No benchmark runs of this version yet.".to_string();
    }
    runs.sort_by_key(|r| r.run_index);
    let rates: Vec<f64> = runs
        .iter()
        .map(|r| crate::bench::pass_rate(r.passed, r.total))
        .collect();
    let median = crate::verdict::median(&rates);
    let mut out = format!(
        "{} runs of this version, median pass rate {:.0}%.\n",
        runs.len(),
        median * 100.0
    );
    let held_out_ids: std::collections::HashSet<&str> = tasks
        .map(|t| {
            t.task
                .iter()
                .filter(|t| !t.split.is_train())
                .map(|t| t.id.as_str())
                .collect()
        })
        .unwrap_or_default();
    // task id -> (runs, passes, first note), in task order of the first run
    let mut order: Vec<String> = Vec::new();
    let mut stats: std::collections::HashMap<String, (u32, u32, Option<String>)> =
        std::collections::HashMap::new();
    for r in &runs {
        for t in r
            .tasks
            .iter()
            .filter(|t| !t.held_out && !held_out_ids.contains(t.id.as_str()))
        {
            let e = stats.entry(t.id.clone()).or_insert_with(|| {
                order.push(t.id.clone());
                (0, 0, None)
            });
            e.0 += 1;
            if t.status == crate::event::TaskStatus::Passed {
                e.1 += 1;
            } else if e.2.is_none() {
                e.2 = t.detail.clone();
            }
        }
    }
    if order.is_empty() {
        out.push_str("No per task results were recorded.\n");
        return out;
    }
    let always: Vec<&String> = order
        .iter()
        .filter(|id| stats[*id].0 == stats[*id].1)
        .collect();
    let mut failing: Vec<&String> = order
        .iter()
        .filter(|id| stats[*id].0 != stats[*id].1)
        .collect();
    // most failures first, then task order
    failing.sort_by_key(|id| std::cmp::Reverse(stats[*id].0 - stats[*id].1));
    if failing.is_empty() {
        out.push_str("Every task passed in every run.\n");
        return crate::failure::head(&out, MAX_RESULTS_BYTES);
    }
    let summary: Vec<String> = failing
        .iter()
        .map(|id| format!("{id} {} of {}", stats[*id].1, stats[*id].0))
        .collect();
    out.push_str(&crate::failure::head(
        &format!(
            "Tasks that did not always pass (passed of runs): {}.\n",
            summary.join(", ")
        ),
        800,
    ));
    out.push_str(&format!("{} tasks passed in every run.\n", always.len()));
    let prompts: std::collections::HashMap<&str, &str> = tasks
        .map(|t| {
            t.task
                .iter()
                .filter(|t| t.split.is_train())
                .map(|t| (t.id.as_str(), t.prompt.as_str()))
                .collect()
        })
        .unwrap_or_default();
    let mut shown = 0usize;
    for id in &failing {
        if shown == MAX_DETAILED_TASKS {
            break;
        }
        let (n, ok, note) = &stats[*id];
        let mut block = format!("\n### {id} (failed {} of {n} runs)\n", n - ok);
        if let Some(p) = prompts.get(id.as_str()) {
            block.push_str(&format!(
                "Task: {}\n",
                crate::failure::head(p.trim(), MAX_TASK_PROMPT_BYTES)
            ));
        }
        match note {
            Some(n) => block.push_str(&format!("{}\n", n.trim_end())),
            None => block.push_str("No failure note was recorded.\n"),
        }
        // keep room for the closing "more tasks" line
        if out.len() + block.len() + 80 > MAX_RESULTS_BYTES {
            break;
        }
        out.push_str(&block);
        shown += 1;
    }
    if shown < failing.len() {
        out.push_str(&format!(
            "\n({} more failing tasks not described)\n",
            failing.len() - shown
        ));
    }
    out
}

/// Added to the results section when the task set has held out tasks. It
/// says that they exist, and nothing about them.
pub const HELD_OUT_NOTICE: &str = "\nThe rules are also scored on other tasks that are not shown here. A change is kept only if it \
helps on those too, so prefer a general habit over a fix for one task.\n";

/// The prompt sent for a mutation: the current rules, how they did on the
/// benchmark (with failure notes), the history, and the size limit.
pub fn build_mutation_prompt(
    parent_rules: &str,
    parent_hash: &str,
    tree: &Tree,
    max_change_bytes: usize,
    results: &str,
    agent_limits: &str,
) -> String {
    format!(
        "You are improving the agent rules file of a software project. The AI model that follows \
these rules stays the same. Only the rules text changes.\n\
\n\
Propose ONE small change to the rules below, and say in one or two plain sentences why it should \
help (your hypothesis).\n\
\n\
Read the benchmark results first. They show which tasks failed and what went wrong. Aim your \
change at the most common cause of failure. Write it as a general rule for this project, not as \
an answer to one task.\n\
\n\
Constraints:\n\
- Change at most {max_change_bytes} bytes in total (text removed plus text added counts as the larger of the two).\n\
- Do not repeat a change that appears in the history below.\n\
- Reply with the hypothesis and the complete new rules text.\n\
{agent_limits}\
\n\
## Current rules (version {})\n\
{parent_rules}\n\
## Benchmark results of the current rules\n\
{results}\n\
## History of this lineage\n\
{}\n",
        short(parent_hash),
        history_text(tree)
    )
}

/// The constraint line added when `mutation.one_shot_agent` is on: what the
/// agent cannot do, so the rules do not ask for it.
pub fn agent_limits_line(hidden: &[String]) -> String {
    let mut line = String::from(
        "- The agent reads the rules and one task, then answers in a single reply. It cannot run \
commands, tests or checks, and it never sees the results.",
    );
    if !hidden.is_empty() {
        line.push_str(&format!(
            " It cannot read these hidden paths: {}.",
            hidden.join(", ")
        ));
    }
    line.push_str(" Do not propose rules that need any of this.\n");
    line
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
    /// Whether a refusal is worth another try with a new seed. Refusals for
    /// the token cap are not: the next try would hit the same cap.
    #[serde(skip)]
    pub retryable: bool,
}

/// Ask the model for a change to `opts.parent` and record the result.
///
/// Errors from the model (including a replay miss) are returned and nothing is
/// written. A model answer that breaks a rule is written as a refusal.
pub fn mutate(repo: &Repo, model: &dyn Model, opts: &MutateOptions) -> Result<MutateOutcome> {
    repo.recover_interrupted_write()?;
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
    let tasks = repo.tasks().ok();
    let mut results = results_text(&tree, &parent, tasks.as_ref());
    if tasks.as_ref().is_some_and(|t| t.has_held_out()) {
        results.push_str(HELD_OUT_NOTICE);
    }
    let limits = if config.mutation.one_shot_agent {
        agent_limits_line(
            tasks
                .as_ref()
                .map(|t| t.sandbox.hidden.as_slice())
                .unwrap_or(&[]),
        )
    } else {
        String::new()
    };
    let prompt = build_mutation_prompt(&parent_text, &parent, &tree, limit, &results, &limits);
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
        return refuse(repo, &parent, &provider, opts.seed, 0, reason, limit, false);
    }
    // A model that answers in a form we cannot read is a refused attempt, not
    // a crash: log it with the reason and move on.
    let response = match model.propose_mutation(&request) {
        Ok(r) => r,
        Err(Error::BadReply(why)) => {
            return refuse(repo, &parent, &provider, opts.seed, 0, why, limit, true);
        }
        Err(e) => return Err(e),
    };
    if response.tokens_used > cap {
        let reason = format!(
            "the model reported {} tokens, over the cap of {cap}, so its answer was thrown away",
            response.tokens_used
        );
        return refuse(
            repo, &parent, &provider, opts.seed, cap, reason, limit, false,
        );
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
    let retryable = result.child.is_none();
    Ok(MutateOutcome {
        result,
        provider,
        seed: opts.seed,
        tokens_used: response.tokens_used,
        retryable,
    })
}

#[allow(clippy::too_many_arguments)]
fn refuse(
    repo: &Repo,
    parent: &str,
    provider: &str,
    seed: u64,
    tokens: u64,
    reason: String,
    limit: usize,
    retryable: bool,
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
        retryable,
    })
}

#[derive(Debug, Clone)]
pub struct EvolveOptions {
    pub generations: u32,
    /// Base seed for runs (shared by parent and child) and for mutations.
    pub base_seed: u64,
    /// `generations` is a total for the project, not for this session: every
    /// judged child and every refused attempt already in the log counts, so
    /// running the same command again after a cut off session never goes past
    /// the total. An unjudged child is finished first and counts as one.
    pub total: bool,
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
    /// True when this generation finished a child that an earlier, cut off
    /// session proposed but never judged (v2.7.2). No new mutation was asked
    /// for; `mutation` describes the open child.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub resumed: bool,
    /// Refused attempts before `mutation`, when `retries_on_refusal` allowed
    /// more tries (v2.7.2). Each one is in the log as `mutation_refused`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub refused_attempts: Vec<String>,
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
    /// Train gain without a held out gain. Left out of JSON when 0.
    #[serde(skip_serializing_if = "is_zero")]
    pub overfit: u32,
    pub refused: u32,
    pub total_tokens: u64,
    /// Isolation backend the checks ran with. `None` when no check ran (every
    /// attempt was refused).
    pub isolation: Option<String>,
    /// Layouts where model-written files could shadow hidden tests.
    pub warnings: Vec<crate::shadow::ShadowWarning>,
    pub generations: Vec<Generation>,
    /// What was repaired when the last write to the log had been cut off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovered_log: Option<String>,
    /// With `total`: generations already in the log when this session
    /// started (judged children plus refused attempts). Left out when 0.
    #[serde(skip_serializing_if = "is_zero")]
    pub generations_already_done: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Loop `generations` times: mutate from `current`, run parent and child, and
/// verify. `current` moves only when a child is accepted.
pub fn evolve(repo: &Repo, model: &dyn Model, opts: &EvolveOptions) -> Result<EvolveReport> {
    let recovered_log = repo.recover_interrupted_write()?;
    // Look before anything is recorded, so `error` mode stops the whole loop.
    let warnings = crate::shadow::check_repo(repo)?;
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
        overfit: 0,
        refused: 0,
        total_tokens: 0,
        isolation: None,
        warnings,
        generations: Vec::new(),
        recovered_log,
        generations_already_done: 0,
    };
    let todo = if opts.total {
        let done = first.edges.iter().filter(|e| e.verdict.is_some()).count() + first.refused.len();
        let done = u32::try_from(done).unwrap_or(u32::MAX);
        report.generations_already_done = done;
        opts.generations.saturating_sub(done)
    } else {
        opts.generations
    };

    let run_opts = |version: &str| RunOptions {
        version: version.to_string(),
        repeats: None,
        base_seed: opts.base_seed,
    };
    let retries = repo.config()?.mutation.retries_on_refusal;
    for g in 0..todo {
        let tree = Tree::from_events(&repo.events()?)?;
        let parent = tree.current.clone();
        // Run the parent first, so the mutation prompt can show how it did.
        // Runs that already exist are not run again.
        let parent_runs = run_version(repo, model, &run_opts(&parent))?;
        if let Some(note) = &parent_runs.stopped_at_token_cap {
            return Err(Error::Input(note.clone()));
        }
        let mut tokens = parent_runs.total_tokens;
        let mut refused_attempts = Vec::new();
        // A child of `current` that was proposed but never judged means an
        // earlier session was cut off (a dead server, a reboot). Finish it
        // before asking for anything new.
        let open = tree
            .edges
            .iter()
            .find(|e| e.parent == parent && e.verdict.is_none())
            .cloned();
        let resumed = open.is_some();
        let mutation = match open {
            Some(edge) => MutateOutcome {
                result: crate::bench::ProposeOutcome {
                    outcome: "resumed".into(),
                    parent: parent.clone(),
                    child: Some(edge.child.clone()),
                    hypothesis: edge.hypothesis.clone(),
                    change_bytes: 0,
                    max_change_bytes: repo.config()?.mutation.max_change_bytes,
                    reason: None,
                    event_seq: tree.node(&edge.child).map(|n| n.created_seq).unwrap_or(0),
                },
                provider: model.name(),
                seed: 0,
                tokens_used: 0,
                retryable: false,
            },
            None => {
                let mut attempt_no = 0;
                loop {
                    let t = Tree::from_events(&repo.events()?)?;
                    let attempt = t.edges.len() + t.refused.len();
                    let opts_m = MutateOptions {
                        parent: parent.clone(),
                        seed: mutation_seed(opts.base_seed, attempt),
                    };
                    let m = match mutate(repo, model, &opts_m) {
                        Ok(m) => m,
                        // A replay recorded before retries existed has no
                        // answer for a retry. The refusal already logged
                        // stands, as it did when it was recorded.
                        Err(Error::ReplayMiss(_)) if attempt_no > 0 => {
                            break refused_attempts
                                .pop()
                                .map(|(m, _): (MutateOutcome, String)| m)
                                .expect("a refused attempt came first")
                        }
                        Err(e) => return Err(e),
                    };
                    tokens += m.tokens_used;
                    if m.result.child.is_some() || !m.retryable || attempt_no >= retries {
                        break m;
                    }
                    let reason = m.result.reason.clone().unwrap_or_default();
                    refused_attempts.push((m, reason));
                    attempt_no += 1;
                }
            }
        };
        report
            .isolation
            .get_or_insert(parent_runs.isolation.clone());
        let mut gen = Generation {
            generation: report.generations_already_done + g + 1,
            parent: parent.clone(),
            parent_runs_recorded: parent_runs.runs.len(),
            child_runs_recorded: 0,
            verdict: None,
            current: parent.clone(),
            tokens_used: 0,
            mutation,
            resumed,
            refused_attempts: refused_attempts.into_iter().map(|(_, r)| r).collect(),
        };
        report.refused += gen.refused_attempts.len() as u32;
        match gen.mutation.result.child.clone() {
            None => report.refused += 1,
            Some(child) => {
                let child_runs = run_version(repo, model, &run_opts(&child))?;
                gen.child_runs_recorded = child_runs.runs.len();
                tokens += child_runs.total_tokens;
                if let Some(note) = &child_runs.stopped_at_token_cap {
                    return Err(Error::Input(note.clone()));
                }
                let verdict = verify_child(repo, &child)?;
                match verdict.decision.verdict {
                    VerdictKind::Accepted => report.accepted += 1,
                    VerdictKind::Rejected => report.rejected += 1,
                    VerdictKind::Noise => report.noise += 1,
                    VerdictKind::Overfit => report.overfit += 1,
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
        let p = build_mutation_prompt("base rules\n", &tree.root, &tree, 1000, "R", "");
        assert!(p.contains("at most 1000 bytes"));
        assert!(p.contains("base rules"));
        assert!(p.contains("No attempts yet."));
        // Same inputs, same prompt.
        assert_eq!(
            p,
            build_mutation_prompt("base rules\n", &tree.root, &tree, 1000, "R", "")
        );
    }

    /// Wraps the mock and keeps every mutation prompt it is sent.
    struct Capture {
        inner: MockModel,
        prompts: std::cell::RefCell<Vec<String>>,
    }
    impl Model for Capture {
        fn name(&self) -> String {
            self.inner.name()
        }
        fn kind(&self) -> crate::model::ProviderKind {
            self.inner.kind()
        }
        fn estimate_tokens(&self, r: &crate::model::ModelRequest) -> u64 {
            self.inner.estimate_tokens(r)
        }
        fn complete(&self, r: &crate::model::ModelRequest) -> Result<crate::model::ModelResponse> {
            self.inner.complete(r)
        }
        fn estimate_mutation_tokens(&self, r: &MutationRequest) -> u64 {
            self.inner.estimate_mutation_tokens(r)
        }
        fn propose_mutation(&self, r: &MutationRequest) -> Result<crate::model::MutationResponse> {
            self.prompts.borrow_mut().push(r.prompt.to_string());
            self.inner.propose_mutation(r)
        }
    }

    const SECRET: &str = "# HIDDEN-EXPECTATION-7f3a: the answer must be exactly ok";

    /// A check that prints its own source (like a traceback quoting a test)
    /// and a failure message, then fails unless the answer is ok.
    fn chatty_check(repo: &Repo, extra_lines: usize) {
        let mut s = format!("{SECRET}\ncat check.sh\n");
        for i in 0..extra_lines {
            s.push_str(&format!("echo \"noise line {i} for $1\"\n"));
        }
        s.push_str(
            "echo \"AssertionError: expected ok, got $(cat answers/$1.txt 2>/dev/null)\" >&2\n",
        );
        s.push_str("grep -qx ok \"answers/$1.txt\"\n");
        std::fs::write(repo.dir().join("check.sh"), s).unwrap();
    }

    #[test]
    fn failures_reach_the_mutation_prompt_without_hidden_lines() {
        let (_d, repo) = fixture(MOCK);
        chatty_check(&repo, 0);
        let m = Capture {
            inner: mock_model(&repo),
            prompts: Default::default(),
        };
        evolve(
            &repo,
            &m,
            &EvolveOptions {
                generations: 1,
                base_seed: 3,
                total: false,
            },
        )
        .unwrap();
        // the log keeps a note for each failed attempt, and only for those
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        let root = tree.node(&tree.root).unwrap();
        let mut notes = 0;
        for r in &root.runs {
            for t in &r.tasks {
                match t.status {
                    crate::event::TaskStatus::Passed => assert!(t.detail.is_none()),
                    _ => {
                        let d = t.detail.as_deref().unwrap();
                        assert!(d.contains("AssertionError: expected ok, got no"), "{d}");
                        assert!(d.contains(&format!("Wrote: answers/{}.txt", t.id)), "{d}");
                        assert!(!d.contains("HIDDEN-EXPECTATION"), "{d}");
                        assert!(!d.contains("grep -qx"), "{d}");
                        assert!(d.contains(crate::failure::REDACTED), "{d}");
                        notes += 1;
                    }
                }
            }
        }
        assert!(notes > 0, "the mock should fail some tasks");
        // the parent ran before the mutation, so its prompt has the results
        let prompts = m.prompts.borrow();
        assert_eq!(prompts.len(), 1);
        let p = &prompts[0];
        assert!(
            p.contains("## Benchmark results of the current rules"),
            "{p}"
        );
        assert!(
            p.contains("5 runs of this version, median pass rate"),
            "{p}"
        );
        assert!(
            p.contains("Tasks that did not always pass (passed of runs): "),
            "{p}"
        );
        assert!(p.contains("AssertionError: expected ok, got no"), "{p}");
        assert!(p.contains("Task: p"), "the task prompt is quoted: {p}");
        assert!(!p.contains("HIDDEN-EXPECTATION"), "{p}");
        assert!(!p.contains("grep -qx"), "{p}");
        // the parent's runs were not run a second time
        let runs_of_root = tree.node(&tree.root).unwrap().runs.len();
        assert_eq!(runs_of_root, 5);
    }

    #[test]
    fn results_section_stays_bounded() {
        let (_d, repo) = fixture(MOCK);
        // lots of output per failure, and long task prompts
        chatty_check(&repo, 400);
        let mut tasks = String::from("[sandbox]\nhidden = [\"check.sh\"]\n");
        for i in 0..40 {
            tasks.push_str(&format!(
                "[[task]]\nid = \"t{i}\"\nprompt = \"{}\"\ncheck = \"sh check.sh t{i}\"\n",
                "do the thing ".repeat(60)
            ));
        }
        std::fs::write(repo.store().root().join("tasks.toml"), tasks).unwrap();
        let m = mock_model(&repo);
        crate::bench::run_version(
            &repo,
            &m,
            &RunOptions {
                version: "root".into(),
                repeats: Some(3),
                base_seed: 1,
            },
        )
        .unwrap();
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        let tf = repo.tasks().unwrap();
        let r = results_text(&tree, &tree.root, Some(&tf));
        assert!(r.len() <= MAX_RESULTS_BYTES, "{} bytes", r.len());
        assert!(r.contains("3 runs of this version"));
        assert!(
            r.contains("noise line 399"),
            "the end of the output is kept: {r}"
        );
        assert!(r.contains("more failing tasks not described"), "{r}");
        assert!(r.matches("\n### ").count() <= MAX_DETAILED_TASKS);
        assert!(!r.contains("HIDDEN-EXPECTATION"));
        // every logged note is bounded too
        for run in &tree.node(&tree.root).unwrap().runs {
            for t in &run.tasks {
                if let Some(d) = &t.detail {
                    assert!(d.len() <= crate::failure::MAX_DETAIL_BYTES);
                }
            }
        }
    }

    #[test]
    fn results_without_runs_or_task_detail_say_so() {
        let (_d, repo) = fixture(MOCK);
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        assert_eq!(
            results_text(&tree, &tree.root, None),
            "No benchmark runs of this version yet."
        );
        assert_eq!(
            results_text(&tree, "nope", None),
            "No benchmark runs of this version yet."
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
        let prompt = build_mutation_prompt("x", &tree.root, &tree, 1000, "R", "");
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
        // no held out tasks: no gate, and nothing new in the log
        assert!(g.decision.held_out.is_none());
        assert!(g.held_out_min_paired_wins.is_none());
        let log = std::fs::read_to_string(repo.store().root().join("log.jsonl")).unwrap();
        assert!(!log.contains("held_out"), "{log}");
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        assert_eq!(tree.current, good.result.child.clone().unwrap());
    }

    /// Five train tasks (t0..t4) and three held out tasks (h0..h2). NARROW
    /// helps only the train tasks, GENERAL helps every task.
    fn held_out_fixture() -> (TempDir, Repo) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("AGENTS.md"), "base rules\n").unwrap();
        init(&InitOptions {
            dir: root.to_path_buf(),
            rules_file: None,
            model: "mock-model".into(),
        })
        .unwrap();
        let mut tasks = String::from("[sandbox]\nhidden = [\"check.sh\"]\n");
        let mut mock = String::from(
            "seed = 2\nbase_pass_probability = 0.3\ntokens_per_call = 100\n\
             [[marker]]\ntext = \"NARROW\"\ndelta = 0.6\ntasks = [\"t0\", \"t1\", \"t2\", \"t3\", \"t4\"]\n\
             [[marker]]\ntext = \"GENERAL\"\ndelta = 0.6\n\
             [[mutation]]\nhypothesis = \"add the narrow hint\"\nappend = \"NARROW\"\n",
        );
        let ids: Vec<(String, bool)> = (0..5)
            .map(|i| (format!("t{i}"), false))
            .chain((0..3).map(|i| (format!("h{i}"), true)))
            .collect();
        for (id, held) in &ids {
            let (prompt, split) = if *held {
                (format!("HELDOUT-PROMPT-{id}"), "split = \"held_out\"\n")
            } else {
                (format!("p {id}"), "")
            };
            tasks.push_str(&format!(
                "[[task]]\nid = \"{id}\"\nprompt = \"{prompt}\"\ncheck = \"sh check.sh {id}\"\n{split}"
            ));
            mock.push_str(&format!(
                "[[answer]]\ntask = \"{id}\"\npath = \"answers/{id}.txt\"\npass_content = \"ok\\n\"\nfail_content = \"no\\n\"\n"
            ));
        }
        // Prints its own source (a hidden line) and a message naming the task.
        std::fs::write(
            root.join("check.sh"),
            format!(
                "{SECRET}\ncat check.sh\necho \"AssertionError in $1: expected ok\" >&2\ngrep -qx ok \"answers/$1.txt\"\n"
            ),
        )
        .unwrap();
        std::fs::write(root.join(".rules-evolve/tasks.toml"), tasks).unwrap();
        std::fs::write(root.join(".rules-evolve/mock.toml"), &mock).unwrap();
        let repo = Repo::open(root).unwrap();
        (dir, repo)
    }

    fn run_all(repo: &Repo, versions: &[String]) {
        let m = mock_model(repo);
        for v in versions {
            run_version(
                repo,
                &m,
                &RunOptions {
                    version: v.clone(),
                    repeats: None,
                    base_seed: 0,
                },
            )
            .unwrap();
        }
    }

    #[test]
    fn held_out_tasks_never_reach_the_mutation_prompt() {
        let (_d, repo) = held_out_fixture();
        let m = Capture {
            inner: mock_model(&repo),
            prompts: Default::default(),
        };
        let report = evolve(
            &repo,
            &m,
            &EvolveOptions {
                generations: 2,
                base_seed: 3,
                total: false,
            },
        )
        .unwrap();
        // the narrow hint wins on train only, so it is overfit
        assert_eq!(report.overfit, 1, "{report:?}");
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        // held out outcomes are in the log, marked, and redacted like the others
        let root = tree.node(&tree.root).unwrap();
        let mut held_notes = 0;
        for r in &root.runs {
            assert_eq!(r.total, 5);
            assert_eq!(r.held_out_total, Some(3));
            for t in &r.tasks {
                assert_eq!(t.held_out, t.id.starts_with('h'), "{t:?}");
                if let Some(d) = &t.detail {
                    assert!(!d.contains("HIDDEN-EXPECTATION"), "{d}");
                    assert!(!d.contains("grep -qx"), "{d}");
                    if t.held_out {
                        held_notes += 1;
                    }
                }
            }
        }
        assert!(held_notes > 0, "some held out task should fail at the root");
        let prompts = m.prompts.borrow();
        assert_eq!(prompts.len(), 2);
        for p in prompts.iter() {
            // no held out id, prompt, note or count
            for h in ["h0", "h1", "h2", "HELDOUT", "held out", "held_out"] {
                assert!(!p.contains(h), "{h:?} leaked into the prompt: {p}");
            }
            assert!(!p.contains("overfit"), "{p}");
            assert!(!p.contains("HIDDEN-EXPECTATION"), "{p}");
            assert!(!p.contains("grep -qx"), "{p}");
            // the train results are there, and the notice
            assert!(p.contains("AssertionError in t"), "{p}");
            assert!(p.contains("Task: p t"), "{p}");
            assert!(p.contains(HELD_OUT_NOTICE.trim()), "{p}");
        }
        // the second prompt shows the overfit change as rejected, train numbers only
        assert!(
            prompts[1].contains("- [rejected] add the narrow hint (median"),
            "{}",
            prompts[1]
        );
        // the train median in the prompt is over 5 train tasks: a multiple of 20%
        let line = prompts[0]
            .lines()
            .find(|l| l.contains("runs of this version, median pass rate"))
            .unwrap();
        let pct: f64 = line
            .rsplit(' ')
            .next()
            .unwrap()
            .trim_end_matches(['%', '.'])
            .parse()
            .unwrap();
        assert!((pct / 20.0 - (pct / 20.0).round()).abs() < 1e-9, "{line}");
    }

    #[test]
    fn narrow_train_win_with_flat_held_out_is_overfit_not_accepted() {
        let (_d, repo) = held_out_fixture();
        let narrow = mutate(&repo, &fixed("base rules\nNARROW\n", "narrow"), &opts(1)).unwrap();
        let child = narrow.result.child.clone().unwrap();
        run_all(&repo, &["root".to_string(), child.clone()]);
        let v = verify_child(&repo, &child).unwrap();
        assert_eq!(
            v.decision.verdict,
            VerdictKind::Overfit,
            "{}",
            v.decision.reason
        );
        // the train rule alone would have accepted it
        assert!(v.decision.gain_pp >= 10.0 && v.decision.paired_wins >= 4);
        let h = v.decision.held_out.as_ref().unwrap();
        assert!(!h.passed);
        assert!(h.gain_pp.abs() < 1e-9, "flat held out: {h:?}");
        assert_eq!(v.held_out_min_paired_wins, Some(3));
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        assert_eq!(tree.current, tree.root, "current must not move");
        assert_eq!(tree.summary().overfit, 1);
        let e = tree.edge_for_child(&child).unwrap();
        assert_eq!(e.held_out_paired_wins, Some(h.paired_wins));
        assert!(e.held_out_child_median.is_some());
    }

    #[test]
    fn worse_held_out_is_overfit_too() {
        let (_d, repo) = held_out_fixture();
        let mut mock = std::fs::read_to_string(repo.store().root().join("mock.toml")).unwrap();
        mock.push_str(
            "[[marker]]\ntext = \"NARROW\"\ndelta = -0.3\ntasks = [\"h0\", \"h1\", \"h2\"]\n",
        );
        std::fs::write(repo.store().root().join("mock.toml"), mock).unwrap();
        let narrow = mutate(&repo, &fixed("base rules\nNARROW\n", "narrow"), &opts(1)).unwrap();
        let child = narrow.result.child.clone().unwrap();
        run_all(&repo, &["root".to_string(), child.clone()]);
        let v = verify_child(&repo, &child).unwrap();
        assert_eq!(
            v.decision.verdict,
            VerdictKind::Overfit,
            "{}",
            v.decision.reason
        );
    }

    #[test]
    fn gain_on_train_and_held_out_is_accepted() {
        let (_d, repo) = held_out_fixture();
        let general = mutate(&repo, &fixed("base rules\nGENERAL\n", "general"), &opts(1)).unwrap();
        let child = general.result.child.clone().unwrap();
        run_all(&repo, &["root".to_string(), child.clone()]);
        let v = verify_child(&repo, &child).unwrap();
        assert_eq!(
            v.decision.verdict,
            VerdictKind::Accepted,
            "{}",
            v.decision.reason
        );
        assert!(v.decision.held_out.as_ref().unwrap().passed);
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        assert_eq!(tree.current, child);
    }

    #[test]
    fn held_out_gate_is_configurable() {
        let (_d, repo) = held_out_fixture();
        // a gate of 0 points and 0 wins lets the narrow change through
        let cfg_path = repo.store().root().join("config.toml");
        let cfg = std::fs::read_to_string(&cfg_path)
            .unwrap()
            .replace(
                "held_out_accept_threshold_pp = 10.0",
                "held_out_accept_threshold_pp = 0.0",
            )
            .replace(
                "held_out_min_paired_wins = 3",
                "held_out_min_paired_wins = 0",
            );
        std::fs::write(&cfg_path, cfg).unwrap();
        let narrow = mutate(&repo, &fixed("base rules\nNARROW\n", "narrow"), &opts(1)).unwrap();
        let child = narrow.result.child.clone().unwrap();
        run_all(&repo, &["root".to_string(), child.clone()]);
        let v = verify_child(&repo, &child).unwrap();
        assert_eq!(
            v.decision.verdict,
            VerdictKind::Accepted,
            "{}",
            v.decision.reason
        );
    }

    #[test]
    fn a_large_held_out_set_uses_the_large_set_bar() {
        // The fixture has 3 held out tasks: below the default of 20, so the
        // usual 10 points apply.
        let (_d, repo) = held_out_fixture();
        let narrow = mutate(&repo, &fixed("base rules\nNARROW\n", "narrow"), &opts(1)).unwrap();
        let child = narrow.result.child.clone().unwrap();
        run_all(&repo, &["root".to_string(), child.clone()]);
        let v = verify_child(&repo, &child).unwrap();
        assert_eq!(v.held_out_accept_threshold_pp, Some(10.0));
        assert_eq!(v.decision.verdict, VerdictKind::Overfit);

        // Count 3 held out tasks as a large set, with a large set bar of 0
        // points and a win count of 0: now the flat held out result passes,
        // which it could not under the 10 point bar.
        let (_d2, repo2) = held_out_fixture();
        let cfg_path = repo2.store().root().join("config.toml");
        let cfg = std::fs::read_to_string(&cfg_path)
            .unwrap()
            .replace(
                "held_out_large_set_tasks = 20",
                "held_out_large_set_tasks = 3",
            )
            .replace(
                "held_out_large_set_threshold_pp = 5.0",
                "held_out_large_set_threshold_pp = 0.0",
            )
            .replace(
                "held_out_min_paired_wins = 3",
                "held_out_min_paired_wins = 0",
            );
        std::fs::write(&cfg_path, cfg).unwrap();
        let narrow = mutate(&repo2, &fixed("base rules\nNARROW\n", "narrow"), &opts(1)).unwrap();
        let child = narrow.result.child.clone().unwrap();
        run_all(&repo2, &["root".to_string(), child.clone()]);
        let v = verify_child(&repo2, &child).unwrap();
        assert_eq!(v.held_out_accept_threshold_pp, Some(0.0));
        assert_eq!(
            v.decision.verdict,
            VerdictKind::Accepted,
            "{}",
            v.decision.reason
        );
    }

    #[test]
    fn adding_held_out_tasks_after_runs_asks_for_a_rerun() {
        let (_d, repo) = fixture(MOCK);
        let good = mutate(&repo, &fixed("base rules\nGOOD\n", "good"), &opts(1)).unwrap();
        let child = good.result.child.clone().unwrap();
        run_all(&repo, &["root".to_string(), child.clone()]);
        let path = repo.store().root().join("tasks.toml");
        let mut t = std::fs::read_to_string(&path).unwrap();
        t.push_str(
            "[[task]]\nid = \"h0\"\nprompt = \"x\"\ncheck = \"true\"\nsplit = \"held_out\"\n",
        );
        std::fs::write(&path, t).unwrap();
        let err = verify_child(&repo, &child).unwrap_err().to_string();
        assert!(err.contains("held out"), "{err}");
    }

    #[test]
    fn results_text_skips_tasks_marked_held_out_in_tasks_toml() {
        // runs recorded before a task was marked held out still keep it out
        let (_d, repo) = fixture(MOCK);
        chatty_check(&repo, 0);
        run_all(&repo, &["root".to_string()]);
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        let mut tf = repo.tasks().unwrap();
        for t in &mut tf.task {
            if t.id != "t0" {
                t.split = crate::tasks::Split::HeldOut;
            }
        }
        let r = results_text(&tree, &tree.root, Some(&tf));
        for i in 1..N {
            assert!(!r.contains(&format!("t{i}")), "{r}");
        }
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
                total: false,
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
                    total: false,
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
