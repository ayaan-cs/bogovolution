//! The benchmark harness: `run`, `verify` and `propose`.
//!
//! Everything here works through the [`Model`] trait, so Phase 2 runs only
//! with the mock model and makes no network calls.

use crate::config::Config;
use crate::error::{Error, Result};
use crate::event::{
    MutationProposedPayload, MutationRefusedPayload, Payload, RunRecordedPayload, TaskOutcome,
    TaskStatus, VerdictPayload,
};
use crate::hash::sha256_hex;
use crate::isolation::Runner;
use crate::model::{Model, ModelRequest};
use crate::repo::Repo;
use crate::sandbox::{CheckStatus, Sandbox};
use crate::shadow::{self, ShadowWarning};
use crate::tasks::TasksFile;
use crate::tree::Tree;
use crate::verdict::{decide, decide_with_held_out, Decision, HeldOutRule, Rule};
use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;

/// Seed of run `run_index` for a base seed. Parent and child use the same
/// function, so run `i` of each shares a seed. Seeds are below 2^48 so they
/// survive JSON readers that store numbers as doubles.
pub fn run_seed(base_seed: u64, run_index: u32) -> u64 {
    let d = sha256_hex(format!("rules-evolve/run/{base_seed}/{run_index}").as_bytes());
    u64::from_str_radix(&d[..12], 16).expect("hex")
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    /// `root`, `current`, a hash, or a unique hash prefix.
    pub version: String,
    /// Number of runs wanted. Falls back to `config.evaluation.repeats`.
    pub repeats: Option<u32>,
    pub base_seed: u64,
}

/// One recorded run, as reported to the caller.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunDetail {
    pub run_index: u32,
    pub seed: u64,
    pub passed: u32,
    pub total: u32,
    pub pass_rate: f64,
    pub tokens_used: u64,
    pub budget_exhausted: bool,
    /// Held out tasks passed and run, when there are held out tasks.
    /// `passed`, `total` and `pass_rate` then count train tasks only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out_passed: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out_total: Option<u32>,
    pub tasks: Vec<TaskOutcome>,
    /// Sequence number of the `run_recorded` event.
    pub event_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunReport {
    pub version: String,
    pub provider: String,
    pub base_seed: u64,
    pub repeats: u32,
    pub max_tokens_per_run: u64,
    /// Isolation backend the checks ran with: `bwrap`, `docker`, `podman` or
    /// `none` (unsafe).
    pub isolation: String,
    /// Layouts where model-written files could shadow hidden tests. Empty when
    /// there are none or `shadow_warnings = "off"`.
    pub warnings: Vec<ShadowWarning>,
    /// Run indexes that were already recorded and so were not run again.
    pub skipped_existing: Vec<u32>,
    pub runs: Vec<RunDetail>,
    pub total_tokens: u64,
}

pub(crate) fn pass_rate(passed: u32, total: u32) -> f64 {
    if total == 0 {
        0.0
    } else {
        passed as f64 / total as f64
    }
}

/// Run one rules version `repeats` times over the whole task set and record
/// each run. Runs that are already in the log are skipped, so an interrupted
/// command can be started again.
pub fn run_version(repo: &Repo, model: &dyn Model, opts: &RunOptions) -> Result<RunReport> {
    let config = repo.config()?;
    let tasks = repo.tasks()?;
    if tasks.task.is_empty() {
        return Err(Error::Input(
            "tasks.toml has no tasks. Add at least one [[task]]".into(),
        ));
    }
    let tree = Tree::from_events(&repo.events()?)?;
    let version = tree.resolve(&opts.version)?;
    let repeats = opts.repeats.unwrap_or(config.evaluation.repeats);
    if repeats == 0 {
        return Err(Error::Input("repeats must be at least 1".into()));
    }
    let cap = config.budget.max_tokens_per_run;
    let rules_bytes = repo.store().get_blob(&version)?;
    let rules_text = String::from_utf8_lossy(&rules_bytes).to_string();
    // The policy lives here: a real provider cannot get a runner without
    // isolation, and this is the only place checks are started from.
    let runner = Runner::resolve(&repo.sandbox_settings()?, model.kind())?;
    let isolation = runner.backend().as_str().to_string();
    let sandbox = Sandbox::scan(repo.dir(), &tree.rules_file, &tasks.sandbox)?.with_runner(runner);
    let hidden_lines = crate::failure::HiddenLines::from_texts(
        sandbox.hidden_texts(crate::failure::MAX_HIDDEN_TEXT_BYTES),
    );
    // Warn about shadow risks, or stop before anything is recorded.
    let settings = repo.sandbox_settings()?;
    let warnings = if settings.shadow_warnings == shadow::ShadowMode::Off {
        Vec::new()
    } else {
        shadow::apply_mode(settings.shadow_warnings, shadow::analyze(&sandbox, &tasks))?
    };

    let existing: HashMap<u32, Option<u64>> = tree
        .node(&version)
        .map(|n| n.runs.iter().map(|r| (r.run_index, r.seed)).collect())
        .unwrap_or_default();

    let mut report = RunReport {
        version: version.clone(),
        provider: model.name(),
        base_seed: opts.base_seed,
        repeats,
        max_tokens_per_run: cap,
        isolation: isolation.clone(),
        warnings,
        skipped_existing: Vec::new(),
        runs: Vec::new(),
        total_tokens: 0,
    };

    for run_index in 0..repeats {
        let seed = run_seed(opts.base_seed, run_index);
        if let Some(old_seed) = existing.get(&run_index) {
            if *old_seed != Some(seed) {
                return Err(Error::Input(format!(
                    "run {run_index} of this version was recorded with a different seed. Use the same --seed as before"
                )));
            }
            report.skipped_existing.push(run_index);
            continue;
        }
        let outcome = execute_run(
            model,
            &tree.model,
            &tasks,
            &sandbox,
            &hidden_lines,
            &rules_text,
            &version,
            run_index,
            seed,
            cap,
        )?;
        let count = |held: bool| {
            let of_split = outcome.tasks.iter().filter(|t| t.held_out == held);
            let total = of_split.clone().count() as u32;
            let passed = of_split.filter(|t| t.status == TaskStatus::Passed).count() as u32;
            (passed, total)
        };
        let (passed, total) = count(false);
        let (held_out_passed, held_out_total) = if tasks.has_held_out() {
            let (p, t) = count(true);
            (Some(p), Some(t))
        } else {
            (None, None)
        };
        let event = repo.append(&Payload::RunRecorded(RunRecordedPayload {
            node: version.clone(),
            run_index,
            passed,
            total,
            tokens_used: outcome.tokens_used,
            seed: Some(seed),
            provider: Some(model.name()),
            budget_exhausted: outcome.budget_exhausted,
            tasks: outcome.tasks.clone(),
            isolation: Some(isolation.clone()),
            held_out_passed,
            held_out_total,
        }))?;
        report.total_tokens += outcome.tokens_used;
        report.runs.push(RunDetail {
            run_index,
            seed,
            passed,
            total,
            pass_rate: pass_rate(passed, total),
            tokens_used: outcome.tokens_used,
            budget_exhausted: outcome.budget_exhausted,
            held_out_passed,
            held_out_total,
            tasks: outcome.tasks,
            event_seq: event.seq,
        });
    }
    Ok(report)
}

struct RunOutcome {
    tasks: Vec<TaskOutcome>,
    tokens_used: u64,
    budget_exhausted: bool,
}

/// One run: every task once, under the token cap.
#[allow(clippy::too_many_arguments)]
fn execute_run(
    model: &dyn Model,
    model_name: &str,
    tasks: &TasksFile,
    sandbox: &Sandbox,
    hidden: &crate::failure::HiddenLines,
    rules_text: &str,
    rules_hash: &str,
    run_index: u32,
    seed: u64,
    cap: u64,
) -> Result<RunOutcome> {
    let mut used: u64 = 0;
    let mut exhausted = false;
    let mut outcomes = Vec::with_capacity(tasks.task.len());

    for task in &tasks.task {
        let mut outcome = TaskOutcome {
            id: task.id.clone(),
            status: TaskStatus::Failed,
            tokens_used: 0,
            detail: None,
            held_out: task.split == crate::tasks::Split::HeldOut,
        };
        if exhausted {
            outcome.status = TaskStatus::SkippedBudget;
            outcomes.push(outcome);
            continue;
        }
        let remaining = cap - used;
        let worktree = sandbox.prepare(rules_text.as_bytes())?;
        let request = ModelRequest {
            model: model_name,
            task_id: &task.id,
            prompt: &task.prompt,
            rules: rules_text,
            rules_hash,
            workdir: worktree.path(),
            seed,
            run_index,
            max_tokens: remaining,
        };
        // Stop before going over the cap.
        if model.estimate_tokens(&request) > remaining {
            exhausted = true;
            outcome.status = TaskStatus::SkippedBudget;
            outcomes.push(outcome);
            continue;
        }
        let response = match model.complete(&request) {
            Ok(r) => r,
            // A replay file that lacks an answer is not a failed task. The
            // whole run is invalid, so stop and say so.
            Err(e @ Error::ReplayMiss(_)) => return Err(e),
            Err(e) => {
                outcome.status = TaskStatus::ModelError;
                outcome.detail = Some(crate::failure::model_error_note(&e.to_string()));
                outcomes.push(outcome);
                continue;
            }
        };
        if response.tokens_used > remaining {
            // The model broke its budget. Throw the answer away and stop.
            used = cap;
            exhausted = true;
            outcome.status = TaskStatus::OverBudget;
            outcome.tokens_used = remaining;
            outcomes.push(outcome);
            continue;
        }
        used += response.tokens_used;
        outcome.tokens_used = response.tokens_used;
        if let Err(why) = worktree.apply_edits(&response.edits) {
            outcome.status = TaskStatus::Violation;
            outcome.detail = Some(crate::failure::violation_note(&why));
            outcomes.push(outcome);
            continue;
        }
        worktree.restore_hidden()?;
        let secs = tasks.timeout_for(task);
        let (status, output) =
            worktree.run_check_captured(&task.check, Duration::from_secs(secs))?;
        outcome.status = match status {
            CheckStatus::Passed => TaskStatus::Passed,
            CheckStatus::Failed => TaskStatus::Failed,
            CheckStatus::TimedOut => TaskStatus::Timeout,
        };
        if outcome.status != TaskStatus::Passed {
            let files: Vec<String> = response.edits.iter().map(|e| e.path.clone()).collect();
            let wt = worktree.path().to_string_lossy().to_string();
            outcome.detail = Some(crate::failure::check_note(
                &files,
                (status == CheckStatus::TimedOut).then_some(secs),
                &output,
                hidden,
                &[wt.as_str(), crate::isolation::WORK_DIR],
            ));
        }
        outcomes.push(outcome);
    }
    Ok(RunOutcome {
        tasks: outcomes,
        tokens_used: used,
        budget_exhausted: exhausted,
    })
}

/// Result of `verify`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VerifyOutcome {
    pub parent: String,
    pub child: String,
    pub accept_threshold_pp: f64,
    pub min_paired_wins: u32,
    /// The held out gate, when it was applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out_accept_threshold_pp: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out_min_paired_wins: Option<u32>,
    pub decision: Decision,
    /// Sequence number of the `verdict` event.
    pub event_seq: u64,
}

/// Compare a child to its parent with the fixed public rule and record the
/// verdict. Both need runs `0..repeats` recorded with matching seeds.
pub fn verify_child(repo: &Repo, child_spec: &str) -> Result<VerifyOutcome> {
    let config = repo.config()?;
    let tree = Tree::from_events(&repo.events()?)?;
    let child = tree.resolve(child_spec)?;
    let edge = tree
        .edge_for_child(&child)
        .ok_or_else(|| Error::Input("the root version has no parent to compare against".into()))?;
    if let Some(v) = edge.verdict {
        return Err(Error::Input(format!(
            "this version already has a verdict ({})",
            v.as_str()
        )));
    }
    let parent = edge.parent.clone();
    let repeats = config.evaluation.repeats;
    if config.evaluation.min_paired_wins > repeats {
        return Err(Error::Input(format!(
            "min_paired_wins ({}) is more than repeats ({repeats}), so nothing could ever be accepted",
            config.evaluation.min_paired_wins
        )));
    }

    if config.evaluation.held_out_min_paired_wins > repeats {
        return Err(Error::Input(format!(
            "held_out_min_paired_wins ({}) is more than repeats ({repeats}), so nothing could ever be accepted",
            config.evaluation.held_out_min_paired_wins
        )));
    }
    let tasks_have_held_out = repo.tasks()?.has_held_out();

    let mut parent_rates = Vec::new();
    let mut child_rates = Vec::new();
    let mut held_parent = Vec::new();
    let mut held_child = Vec::new();
    let mut runs_have_held_out = Vec::new();
    for run_index in 0..repeats {
        let find = |hash: &str| {
            tree.node(hash)
                .and_then(|n| n.runs.iter().find(|r| r.run_index == run_index))
        };
        let (p, c) = match (find(&parent), find(&child)) {
            (Some(p), Some(c)) => (p, c),
            (p, c) => {
                let missing = match (p.is_none(), c.is_none()) {
                    (true, true) => "parent and child",
                    (true, false) => "parent",
                    _ => "child",
                };
                return Err(Error::Input(format!(
                    "run {run_index} is missing for the {missing}. Run both versions with `run` first ({repeats} runs each)"
                )));
            }
        };
        if p.seed.is_none() || p.seed != c.seed {
            return Err(Error::Input(format!(
                "run {run_index} was not run with the same seed for parent and child, so the pair is not comparable"
            )));
        }
        if p.total == 0 || c.total == 0 || p.total != c.total {
            return Err(Error::Input(format!(
                "run {run_index} used different task counts for parent and child, so the pair is not comparable"
            )));
        }
        parent_rates.push(pass_rate(p.passed, p.total));
        child_rates.push(pass_rate(c.passed, c.total));
        for r in [p, c] {
            runs_have_held_out.push(r.held_out_total.is_some_and(|t| t > 0));
        }
        if let (Some(pp), Some(pt), Some(cp), Some(ct)) = (
            p.held_out_passed,
            p.held_out_total,
            c.held_out_passed,
            c.held_out_total,
        ) {
            if pt > 0 && pt == ct {
                held_parent.push(pass_rate(pp, pt));
                held_child.push(pass_rate(cp, ct));
            }
        }
    }
    let all_held = runs_have_held_out.iter().all(|&b| b);
    let any_held = runs_have_held_out.iter().any(|&b| b);
    if (tasks_have_held_out || any_held) && !(all_held && held_parent.len() == repeats as usize) {
        return Err(Error::Input(
            "tasks.toml has held out tasks, but not every paired run of the parent and child scored the same held out tasks. Rerun the versions with `run` (a new seed, or a fresh project) so the held out gate can be applied".into(),
        ));
    }

    let rule = Rule {
        accept_threshold_pp: config.evaluation.accept_threshold_pp,
        min_paired_wins: config.evaluation.min_paired_wins,
    };
    let gate = HeldOutRule {
        accept_threshold_pp: config.evaluation.held_out_accept_threshold_pp,
        min_paired_wins: config.evaluation.held_out_min_paired_wins,
    };
    let decision = if all_held && !held_parent.is_empty() {
        decide_with_held_out(
            &parent_rates,
            &child_rates,
            &rule,
            &held_parent,
            &held_child,
            &gate,
        )
    } else {
        decide(&parent_rates, &child_rates, &rule)
    }
    .map_err(|e| Error::Input(e.to_string()))?;
    let event = repo.append(&Payload::Verdict(VerdictPayload {
        parent: parent.clone(),
        child: child.clone(),
        verdict: decision.verdict,
        parent_median: Some(decision.parent_median),
        child_median: Some(decision.child_median),
        paired_wins: Some(decision.paired_wins),
        repeats: Some(decision.repeats),
        note: Some(decision.reason.clone()),
        held_out_parent_median: decision.held_out.as_ref().map(|h| h.parent_median),
        held_out_child_median: decision.held_out.as_ref().map(|h| h.child_median),
        held_out_paired_wins: decision.held_out.as_ref().map(|h| h.paired_wins),
    }))?;
    Ok(VerifyOutcome {
        parent,
        child,
        accept_threshold_pp: rule.accept_threshold_pp,
        min_paired_wins: rule.min_paired_wins,
        held_out_accept_threshold_pp: decision.held_out.as_ref().map(|_| gate.accept_threshold_pp),
        held_out_min_paired_wins: decision.held_out.as_ref().map(|_| gate.min_paired_wins),
        decision,
        event_seq: event.seq,
    })
}

/// Bytes that differ between two texts, found by trimming the shared start and
/// end. This is the size of the edited middle: the larger of what was cut out
/// and what was put in.
pub fn change_bytes(old: &[u8], new: &[u8]) -> usize {
    let prefix = old
        .iter()
        .zip(new.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let max_suffix = old.len().min(new.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    (old.len() - prefix - suffix).max(new.len() - prefix - suffix)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProposeOutcome {
    /// `proposed` or `refused`.
    pub outcome: String,
    pub parent: String,
    /// Hash of the new version. `None` when refused.
    pub child: Option<String>,
    pub hypothesis: String,
    pub change_bytes: usize,
    pub max_change_bytes: usize,
    pub reason: Option<String>,
    pub event_seq: u64,
}

/// Where a mutation came from, kept in the log.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MutationOrigin {
    pub seed: Option<u64>,
    pub provider: Option<String>,
    pub tokens_used: Option<u64>,
}

/// Check a proposed change and write either `mutation_proposed` or
/// `mutation_refused`. Refusal reasons: empty hypothesis, empty rules, the same
/// text as the parent or as any version already in the tree, or a change over
/// the size limit.
pub fn record_mutation(
    repo: &Repo,
    config: &Config,
    parent: &str,
    content: &[u8],
    hypothesis: &str,
    origin: &MutationOrigin,
) -> Result<ProposeOutcome> {
    let tree = Tree::from_events(&repo.events()?)?;
    let parent_bytes = repo.store().get_blob(parent)?;
    let child = sha256_hex(content);
    let size = change_bytes(&parent_bytes, content);
    let limit = config.mutation.max_change_bytes;

    let refusal = if hypothesis.trim().is_empty() {
        Some("the hypothesis is empty".to_string())
    } else if content.iter().all(|b| b.is_ascii_whitespace()) {
        Some("the new rules text is empty".to_string())
    } else if child == parent {
        Some("the new text is identical to the parent".to_string())
    } else if tree.node(&child).is_some() {
        Some(format!(
            "this content already exists in the tree as {}",
            &child[..12]
        ))
    } else if size > limit {
        Some(format!(
            "change of {size} bytes is over the limit of {limit}"
        ))
    } else {
        None
    };

    if let Some(reason) = refusal {
        let event = repo.append(&Payload::MutationRefused(MutationRefusedPayload {
            parent: parent.to_string(),
            hypothesis: if hypothesis.trim().is_empty() {
                None
            } else {
                Some(hypothesis.to_string())
            },
            reason: reason.clone(),
            seed: origin.seed,
            provider: origin.provider.clone(),
            tokens_used: origin.tokens_used,
        }))?;
        return Ok(ProposeOutcome {
            outcome: "refused".into(),
            parent: parent.to_string(),
            child: None,
            hypothesis: hypothesis.to_string(),
            change_bytes: size,
            max_change_bytes: limit,
            reason: Some(reason),
            event_seq: event.seq,
        });
    }

    repo.store().put_blob(content)?;
    let event = repo.append(&Payload::MutationProposed(MutationProposedPayload {
        parent: parent.to_string(),
        child: child.clone(),
        hypothesis: hypothesis.to_string(),
        seed: origin.seed,
        provider: origin.provider.clone(),
        tokens_used: origin.tokens_used,
    }))?;
    Ok(ProposeOutcome {
        outcome: "proposed".into(),
        parent: parent.to_string(),
        child: Some(child),
        hypothesis: hypothesis.to_string(),
        change_bytes: size,
        max_change_bytes: limit,
        reason: None,
        event_seq: event.seq,
    })
}

/// Put a new rules version into the tree from a file. This is the hand made
/// counterpart of the model driven `mutate`. It enforces the same checks and
/// records refusals in the log.
pub fn propose(
    repo: &Repo,
    config: &Config,
    parent_spec: &str,
    content: &[u8],
    hypothesis: &str,
) -> Result<ProposeOutcome> {
    if hypothesis.trim().is_empty() {
        return Err(Error::Input("a mutation needs a hypothesis".into()));
    }
    if std::str::from_utf8(content).is_err() {
        return Err(Error::Input("the rules file must be UTF-8 text".into()));
    }
    let tree = Tree::from_events(&repo.events()?)?;
    let parent = tree.resolve(parent_spec)?;
    record_mutation(
        repo,
        config,
        &parent,
        content,
        hypothesis,
        &MutationOrigin::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::VerdictKind;
    use crate::init::{init, InitOptions};
    use crate::model::{FileEdit, MockConfig, MockModel, ModelResponse};
    use std::cell::RefCell;
    use std::path::Path;
    use tempfile::{tempdir, TempDir};

    const N_TASKS: usize = 6;

    /// A tiny repo: each task `tN` passes when `answers/tN.txt` says `ok`.
    /// The checking script is hidden from the model.
    fn fixture(mock_toml: &str) -> (TempDir, Repo) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("AGENTS.md"), "base rules\n").unwrap();
        std::fs::write(root.join("README.txt"), "a tiny project\n").unwrap();
        std::fs::write(root.join("check.sh"), "grep -qx ok \"answers/$1.txt\"\n").unwrap();
        init(&InitOptions {
            dir: root.to_path_buf(),
            rules_file: None,
            model: "mock-model".into(),
        })
        .unwrap();
        let mut tasks = String::from("[sandbox]\nhidden = [\"check.sh\"]\n");
        for i in 0..N_TASKS {
            tasks.push_str(&format!(
                "[[task]]\nid = \"t{i}\"\nprompt = \"solve task {i}\"\ncheck = \"sh check.sh t{i}\"\n"
            ));
        }
        std::fs::write(root.join(".rules-evolve/tasks.toml"), tasks).unwrap();
        std::fs::write(root.join(".rules-evolve/mock.toml"), mock_toml).unwrap();
        let repo = Repo::open(root).unwrap();
        (dir, repo)
    }

    fn mock_toml(base: f64, markers: &str) -> String {
        let mut t =
            format!("seed = 5\nbase_pass_probability = {base}\ntokens_per_call = 100\n{markers}");
        for i in 0..N_TASKS {
            t.push_str(&format!(
                "[[answer]]\ntask = \"t{i}\"\npath = \"answers/t{i}.txt\"\npass_content = \"ok\\n\"\nfail_content = \"nope\\n\"\n"
            ));
        }
        t
    }

    fn mock(repo: &Repo) -> MockModel {
        MockModel::new(MockConfig::load(&repo.store().root().join("mock.toml")).unwrap())
    }

    fn opts(version: &str) -> RunOptions {
        RunOptions {
            version: version.into(),
            repeats: None,
            base_seed: 0,
        }
    }

    fn set_cap(repo: &Repo, cap: u64) {
        let p = repo.store().config_path();
        let t = std::fs::read_to_string(&p).unwrap();
        std::fs::write(
            &p,
            t.replace(
                "max_tokens_per_run = 200000",
                &format!("max_tokens_per_run = {cap}"),
            ),
        )
        .unwrap();
    }

    fn propose_text(repo: &Repo, text: &str, hyp: &str) -> String {
        let cfg = repo.config().unwrap();
        let out = propose(repo, &cfg, "root", text.as_bytes(), hyp).unwrap();
        out.child
            .unwrap_or_else(|| panic!("refused: {:?}", out.reason))
    }

    #[test]
    fn run_records_five_runs_with_seed_and_tokens() {
        let (_d, repo) = fixture(&mock_toml(1.0, ""));
        let report = run_version(&repo, &mock(&repo), &opts("root")).unwrap();
        assert_eq!(report.runs.len(), 5);
        assert_eq!(report.provider, "mock");
        for (i, r) in report.runs.iter().enumerate() {
            assert_eq!(r.run_index, i as u32);
            assert_eq!(r.passed, N_TASKS as u32);
            assert_eq!(r.total, N_TASKS as u32);
            assert_eq!(r.seed, run_seed(0, i as u32));
            assert!(r.tokens_used > 0);
            assert!(!r.budget_exhausted);
        }
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        let node = tree.node(&tree.root).unwrap();
        assert_eq!(node.runs.len(), 5);
        assert_eq!(node.runs[2].seed, Some(run_seed(0, 2)));
        assert_eq!(node.runs[2].provider.as_deref(), Some("mock"));
    }

    #[test]
    fn failing_model_fails_the_checks() {
        let (_d, repo) = fixture(&mock_toml(0.0, ""));
        let report = run_version(&repo, &mock(&repo), &opts("root")).unwrap();
        assert!(report.runs.iter().all(|r| r.passed == 0));
        assert!(report.runs[0]
            .tasks
            .iter()
            .all(|t| t.status == TaskStatus::Failed));
    }

    #[test]
    fn rerun_skips_recorded_runs_and_checks_the_seed() {
        let (_d, repo) = fixture(&mock_toml(1.0, ""));
        let m = mock(&repo);
        let mut o = opts("root");
        o.repeats = Some(2);
        run_version(&repo, &m, &o).unwrap();
        o.repeats = Some(4);
        let r = run_version(&repo, &m, &o).unwrap();
        assert_eq!(r.skipped_existing, vec![0, 1]);
        assert_eq!(r.runs.len(), 2);
        o.base_seed = 99;
        assert!(run_version(&repo, &m, &o).is_err());
    }

    #[test]
    fn token_cap_stops_a_run_before_going_over() {
        // Each call costs 100 + a few tokens. A cap of 250 fits two calls.
        let (_d, repo) = fixture(&mock_toml(1.0, ""));
        set_cap(&repo, 250);
        let report = run_version(&repo, &mock(&repo), &opts("root")).unwrap();
        for r in &report.runs {
            assert!(r.tokens_used <= 250, "{}", r.tokens_used);
            assert!(r.budget_exhausted);
            assert_eq!(r.passed, 2);
            assert_eq!(r.total, N_TASKS as u32);
            let skipped = r
                .tasks
                .iter()
                .filter(|t| t.status == TaskStatus::SkippedBudget)
                .count();
            assert_eq!(skipped, N_TASKS - 2);
        }
    }

    #[test]
    fn a_cap_too_small_for_one_call_runs_nothing() {
        let (_d, repo) = fixture(&mock_toml(1.0, ""));
        set_cap(&repo, 50);
        let report = run_version(&repo, &mock(&repo), &opts("root")).unwrap();
        for r in &report.runs {
            assert_eq!(r.tokens_used, 0);
            assert_eq!(r.passed, 0);
            assert!(r.budget_exhausted);
        }
    }

    /// A model that claims far more tokens than the estimate said.
    struct Overspender;
    impl Model for Overspender {
        fn name(&self) -> String {
            "overspender".into()
        }

        fn kind(&self) -> crate::model::ProviderKind {
            crate::model::ProviderKind::Local
        }
        fn estimate_tokens(&self, _r: &ModelRequest) -> u64 {
            10
        }
        fn complete(&self, _r: &ModelRequest) -> Result<ModelResponse> {
            Ok(ModelResponse {
                edits: vec![FileEdit {
                    path: "answers/t0.txt".into(),
                    content: "ok\n".into(),
                }],
                tokens_used: 1_000_000,
            })
        }
    }

    #[test]
    fn a_model_that_overspends_is_cut_off_at_the_cap() {
        let (_d, repo) = fixture(&mock_toml(1.0, ""));
        set_cap(&repo, 500);
        let report = run_version(&repo, &Overspender, &opts("root")).unwrap();
        let r = &report.runs[0];
        assert_eq!(r.tokens_used, 500);
        assert_eq!(r.tasks[0].status, TaskStatus::OverBudget);
        assert_eq!(r.tasks[1].status, TaskStatus::SkippedBudget);
        assert_eq!(r.passed, 0);
        assert!(r.budget_exhausted);
    }

    /// Looks for the benchmark files while "thinking", and records what it saw.
    struct Spy {
        saw: RefCell<Vec<String>>,
    }
    impl Model for Spy {
        fn name(&self) -> String {
            "spy".into()
        }

        fn kind(&self) -> crate::model::ProviderKind {
            crate::model::ProviderKind::Local
        }
        fn estimate_tokens(&self, _r: &ModelRequest) -> u64 {
            1
        }
        fn complete(&self, r: &ModelRequest) -> Result<ModelResponse> {
            for probe in [
                "check.sh",
                ".rules-evolve",
                ".git",
                ".rules-evolve/tasks.toml",
            ] {
                if r.workdir.join(probe).exists() {
                    self.saw.borrow_mut().push(probe.to_string());
                }
            }
            // The tests are not in the folder, and the version under test is.
            assert!(Path::new(r.workdir).join("README.txt").exists());
            assert_eq!(
                std::fs::read_to_string(r.workdir.join("AGENTS.md")).unwrap(),
                r.rules
            );
            let mut edits = vec![FileEdit {
                path: format!("answers/{}.txt", r.task_id),
                content: "ok\n".into(),
            }];
            if r.task_id == "t1" {
                // Tries to overwrite its own check script.
                edits.push(FileEdit {
                    path: "check.sh".into(),
                    content: "exit 0\n".into(),
                });
            }
            Ok(ModelResponse {
                edits,
                tokens_used: 1,
            })
        }
    }

    #[test]
    fn harness_never_exposes_check_files_to_the_model() {
        let (_d, repo) = fixture(&mock_toml(1.0, ""));
        let spy = Spy {
            saw: RefCell::new(Vec::new()),
        };
        let mut o = opts("root");
        o.repeats = Some(1);
        let report = run_version(&repo, &spy, &o).unwrap();
        assert!(
            spy.saw.borrow().is_empty(),
            "model saw {:?}",
            spy.saw.borrow()
        );
        // The check files were restored for the check, so honest tasks pass.
        let tasks = &report.runs[0].tasks;
        assert_eq!(tasks[0].status, TaskStatus::Passed);
        // The attempt to overwrite check.sh is a violation, and counts as failed.
        assert_eq!(tasks[1].status, TaskStatus::Violation);
        assert_eq!(report.runs[0].passed, (N_TASKS - 1) as u32);
        // The real check script on disk is untouched.
        let on_disk = std::fs::read_to_string(repo.dir().join("check.sh")).unwrap();
        assert!(on_disk.contains("grep"));
    }

    #[test]
    fn check_timeout_counts_as_failure() {
        let (_d, repo) = fixture(&mock_toml(1.0, ""));
        let tasks = "[[task]]\nid=\"slow\"\nprompt=\"p\"\ncheck=\"sleep 20\"\ntimeout_secs=1\n";
        std::fs::write(repo.store().tasks_path(), tasks).unwrap();
        let mut o = opts("root");
        o.repeats = Some(1);
        let report = run_version(&repo, &mock(&repo), &o).unwrap();
        assert_eq!(report.runs[0].tasks[0].status, TaskStatus::Timeout);
        assert_eq!(report.runs[0].passed, 0);
    }

    fn run_both(repo: &Repo, child: &str) {
        let m = mock(repo);
        run_version(repo, &m, &opts("root")).unwrap();
        run_version(repo, &m, &opts(child)).unwrap();
    }

    #[test]
    fn better_rules_are_accepted() {
        let markers = "[[marker]]\ntext = \"RUN THE TESTS\"\ndelta = 0.7\n";
        let (_d, repo) = fixture(&mock_toml(0.2, markers));
        let child = propose_text(&repo, "base rules\nRUN THE TESTS\n", "tell it to run tests");
        run_both(&repo, &child);
        let out = verify_child(&repo, &child).unwrap();
        assert_eq!(
            out.decision.verdict,
            VerdictKind::Accepted,
            "{:?}",
            out.decision
        );
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        assert_eq!(tree.current, child);
        assert_eq!(
            tree.edge_for_child(&child).unwrap().verdict,
            Some(VerdictKind::Accepted)
        );
    }

    #[test]
    fn worse_rules_are_rejected() {
        let markers = "[[marker]]\ntext = \"IGNORE TESTS\"\ndelta = -0.9\n";
        let (_d, repo) = fixture(&mock_toml(0.9, markers));
        let child = propose_text(&repo, "base rules\nIGNORE TESTS\n", "skip tests");
        run_both(&repo, &child);
        let out = verify_child(&repo, &child).unwrap();
        assert_eq!(out.decision.verdict, VerdictKind::Rejected);
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        assert_eq!(tree.current, tree.root);
    }

    #[test]
    fn verify_needs_runs_for_both_versions_and_only_once() {
        let markers = "[[marker]]\ntext = \"X\"\ndelta = 0.5\n";
        let (_d, repo) = fixture(&mock_toml(0.2, markers));
        let child = propose_text(&repo, "base rules\nX\n", "x");
        let err = verify_child(&repo, &child).unwrap_err().to_string();
        assert!(err.contains("missing"), "{err}");
        run_version(&repo, &mock(&repo), &opts("root")).unwrap();
        assert!(verify_child(&repo, &child).is_err());
        run_version(&repo, &mock(&repo), &opts(&child)).unwrap();
        verify_child(&repo, &child).unwrap();
        let again = verify_child(&repo, &child).unwrap_err().to_string();
        assert!(again.contains("already has a verdict"));
        assert!(verify_child(&repo, "root").is_err());
    }

    #[test]
    fn verify_refuses_unpaired_runs() {
        let (_d, repo) = fixture(&mock_toml(0.5, ""));
        let child = propose_text(&repo, "base rules\nmore\n", "m");
        let m = mock(&repo);
        run_version(&repo, &m, &opts("root")).unwrap();
        let mut o = opts(&child);
        o.base_seed = 12345;
        run_version(&repo, &m, &o).unwrap();
        let err = verify_child(&repo, &child).unwrap_err().to_string();
        assert!(err.contains("same seed"), "{err}");
    }

    #[test]
    fn change_size_is_measured_on_the_edited_middle() {
        assert_eq!(change_bytes(b"abc", b"abc"), 0);
        assert_eq!(change_bytes(b"hello world", b"hello brave new world"), 10);
        assert_eq!(change_bytes(b"hello brave new world", b"hello world"), 10);
        assert_eq!(change_bytes(b"", b"abcd"), 4);
        assert_eq!(change_bytes(b"abcd", b""), 4);
        assert_eq!(change_bytes(b"aaa", b"aa"), 1);
    }

    #[test]
    fn propose_enforces_size_limit_and_logs_refusals() {
        let (_d, repo) = fixture(&mock_toml(0.5, ""));
        let cfg = repo.config().unwrap();
        let big = format!(
            "base rules\n{}",
            "x".repeat(cfg.mutation.max_change_bytes + 1)
        );
        let out = propose(&repo, &cfg, "root", big.as_bytes(), "huge").unwrap();
        assert_eq!(out.outcome, "refused");
        assert!(out.child.is_none());
        assert!(out.reason.unwrap().contains("over the limit"));
        // At the limit is fine.
        let ok = format!("base rules\n{}", "x".repeat(cfg.mutation.max_change_bytes));
        let out = propose(&repo, &cfg, "root", ok.as_bytes(), "just right").unwrap();
        assert_eq!(out.outcome, "proposed");
        // The same content again, and the parent itself, are refused.
        assert_eq!(
            propose(&repo, &cfg, "root", ok.as_bytes(), "again")
                .unwrap()
                .outcome,
            "refused"
        );
        assert_eq!(
            propose(&repo, &cfg, "root", b"base rules\n", "same")
                .unwrap()
                .outcome,
            "refused"
        );
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        let s = tree.summary();
        assert_eq!((s.nodes, s.refused), (2, 3));
        assert!(propose(&repo, &cfg, "root", b"y", "  ").is_err());
        assert!(propose(&repo, &cfg, "root", &[0xff, 0xfe], "bad utf8").is_err());
    }

    #[test]
    fn run_seed_is_stable() {
        assert_eq!(run_seed(0, 0), run_seed(0, 0));
        assert_ne!(run_seed(0, 0), run_seed(0, 1));
        assert_ne!(run_seed(0, 0), run_seed(1, 0));
        for i in 0..50 {
            assert!(run_seed(i, i as u32) < (1u64 << 48));
        }
    }

    // ---- isolation (v1.2) ----

    fn set_isolation(repo: &Repo, value: &str) {
        let p = repo.store().config_path();
        let t = std::fs::read_to_string(&p).unwrap();
        let out: Vec<String> = t
            .lines()
            .map(|l| {
                if l.starts_with("isolation = ") {
                    format!("isolation = \"{value}\"")
                } else {
                    l.to_string()
                }
            })
            .collect();
        std::fs::write(&p, out.join("\n") + "\n").unwrap();
    }

    /// Pretends to be a real, networked provider (none exist yet), so the
    /// isolation policy can be tested before one is added.
    struct FakeReal(MockModel);
    impl Model for FakeReal {
        fn name(&self) -> String {
            "fake-real".into()
        }
        fn kind(&self) -> crate::model::ProviderKind {
            crate::model::ProviderKind::Real
        }
        fn estimate_tokens(&self, r: &ModelRequest) -> u64 {
            self.0.estimate_tokens(r)
        }
        fn complete(&self, r: &ModelRequest) -> Result<ModelResponse> {
            self.0.complete(r)
        }
        fn estimate_mutation_tokens(&self, r: &crate::model::MutationRequest) -> u64 {
            self.0.estimate_mutation_tokens(r)
        }
        fn propose_mutation(
            &self,
            r: &crate::model::MutationRequest,
        ) -> Result<crate::model::MutationResponse> {
            self.0.propose_mutation(r)
        }
    }

    fn bwrap_works() -> bool {
        let ok = crate::isolation::probe_backend(
            crate::isolation::Backend::Bwrap,
            &crate::isolation::SandboxSettings::default(),
        );
        if let Err(why) = &ok {
            eprintln!("SKIPPED: bwrap does not work here: {why}");
        }
        ok.is_ok()
    }

    fn recorded_isolation(repo: &Repo) -> Vec<Option<String>> {
        repo.events()
            .unwrap()
            .iter()
            .filter_map(|e| match Payload::from_event(e).unwrap() {
                Payload::RunRecorded(r) => Some(r.isolation),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_real_provider_cannot_run_checks_without_isolation() {
        let (_d, repo) = fixture(&mock_toml(1.0, ""));
        set_isolation(&repo, "none");
        let fake = FakeReal(mock(&repo));
        let before = repo.events().unwrap().len();
        let err = run_version(&repo, &fake, &opts("root"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("real provider must run isolated"), "{err}");
        // Nothing ran and nothing was recorded.
        assert_eq!(repo.events().unwrap().len(), before);
        // The same goes for mutate and evolve, before any model call.
        let m = crate::mutate::mutate(
            &repo,
            &fake,
            &crate::mutate::MutateOptions {
                parent: "root".into(),
                seed: 0,
            },
        );
        assert!(m
            .unwrap_err()
            .to_string()
            .contains("real provider must run isolated"));
        let e = crate::mutate::evolve(
            &repo,
            &fake,
            &crate::mutate::EvolveOptions {
                generations: 1,
                base_seed: 0,
            },
        );
        assert!(e
            .unwrap_err()
            .to_string()
            .contains("real provider must run isolated"));
        assert_eq!(repo.events().unwrap().len(), before);
        // The command line override cannot be used to get around it either.
        let mut repo2 = Repo::open(repo.dir()).unwrap();
        repo2.set_isolation_override(Some(crate::isolation::Isolation::None));
        set_isolation(&repo, "auto");
        assert!(run_version(&repo2, &fake, &opts("root")).is_err());
    }

    #[test]
    fn a_real_provider_runs_when_a_backend_works_and_is_refused_when_none_does() {
        let (_d, repo) = fixture(&mock_toml(1.0, ""));
        let fake = FakeReal(mock(&repo));
        let mut o = opts("root");
        o.repeats = Some(1);
        let r = run_version(&repo, &fake, &o);
        if bwrap_works() {
            let report = r.unwrap();
            assert_eq!(report.isolation, "bwrap");
            assert_eq!(recorded_isolation(&repo), vec![Some("bwrap".to_string())]);
        } else {
            // No bwrap here. Docker and podman may still work.
            match r {
                Ok(report) => assert!(report.isolation == "docker" || report.isolation == "podman"),
                Err(e) => assert!(e.to_string().contains("no isolation backend works")),
            }
        }
    }

    #[test]
    fn each_run_event_records_the_backend_that_was_used() {
        let (_d, repo) = fixture(&mock_toml(1.0, ""));
        set_isolation(&repo, "none");
        let mut o = opts("root");
        o.repeats = Some(2);
        let report = run_version(&repo, &mock(&repo), &o).unwrap();
        assert_eq!(report.isolation, "none");
        assert_eq!(
            recorded_isolation(&repo),
            vec![Some("none".to_string()), Some("none".to_string())]
        );
    }

    #[test]
    fn the_check_runs_inside_the_isolation_and_cannot_see_the_real_repo() {
        if !bwrap_works() {
            return;
        }
        let (d, repo) = fixture(&mock_toml(1.0, ""));
        // A check that passes only when the real repo path is not visible.
        let hidden = d.path().join("check.sh");
        let tasks = format!(
            "[sandbox]\nhidden = [\"check.sh\"]\n[[task]]\nid=\"t0\"\nprompt=\"p\"\ncheck=\"test ! -e '{}' && test -e check.sh\"\n",
            hidden.display()
        );
        std::fs::write(repo.store().tasks_path(), tasks).unwrap();
        let mut o = opts("root");
        o.repeats = Some(1);
        set_isolation(&repo, "bwrap");
        let isolated = run_version(&repo, &mock(&repo), &o).unwrap();
        assert_eq!(isolated.runs[0].tasks[0].status, TaskStatus::Passed);
        // Control: with none, the real path is visible, so the same check fails.
        set_isolation(&repo, "bwrap");
        let mut repo2 = Repo::open(repo.dir()).unwrap();
        repo2.set_isolation_override(Some(crate::isolation::Isolation::None));
        let mut o2 = opts("root");
        o2.repeats = Some(2);
        let open = run_version(&repo2, &mock(&repo), &o2).unwrap();
        assert_eq!(open.isolation, "none");
        assert_eq!(open.runs[0].tasks[0].status, TaskStatus::Failed);
    }

    // ---- shadow warnings ----

    /// The fixture plus a risky layout: a hidden spec beside a visible readme.
    fn risky(repo: &Repo) {
        let root = repo.dir();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/spec.txt"), "spec\n").unwrap();
        std::fs::write(root.join("docs/readme.txt"), "readme\n").unwrap();
        let p = repo.store().tasks_path();
        let t = std::fs::read_to_string(&p).unwrap();
        std::fs::write(
            &p,
            t.replace(
                "hidden = [\"check.sh\"]",
                "hidden = [\"check.sh\", \"docs/spec.txt\"]",
            ),
        )
        .unwrap();
    }

    fn set_shadow(repo: &Repo, mode: &str) {
        let p = repo.store().config_path();
        let t = std::fs::read_to_string(&p).unwrap();
        let out: Vec<String> = t
            .lines()
            .map(|l| {
                if l.starts_with("shadow_warnings = ") {
                    format!("shadow_warnings = \"{mode}\"")
                } else {
                    l.to_string()
                }
            })
            .collect();
        std::fs::write(&p, out.join("\n") + "\n").unwrap();
    }

    #[test]
    fn a_clean_layout_has_no_warnings() {
        let (_d, repo) = fixture(&mock_toml(1.0, ""));
        let mut o = opts("root");
        o.repeats = Some(1);
        let report = run_version(&repo, &mock(&repo), &o).unwrap();
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn a_risky_layout_warns_by_default_and_the_run_still_happens() {
        let (_d, repo) = fixture(&mock_toml(1.0, ""));
        risky(&repo);
        let mut o = opts("root");
        o.repeats = Some(1);
        let report = run_version(&repo, &mock(&repo), &o).unwrap();
        assert_eq!(report.warnings.len(), 1);
        assert_eq!(
            report.warnings[0].kind,
            crate::shadow::ShadowKind::HiddenFileBesideVisibleFiles
        );
        assert_eq!(report.runs.len(), 1);
    }

    #[test]
    fn error_mode_stops_before_anything_is_recorded() {
        let (_d, repo) = fixture(&mock_toml(1.0, ""));
        risky(&repo);
        set_shadow(&repo, "error");
        let before = repo.events().unwrap().len();
        let err = run_version(&repo, &mock(&repo), &opts("root"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("shadow_warnings = \"error\""), "{err}");
        assert!(err.contains("docs/spec.txt"));
        assert_eq!(repo.events().unwrap().len(), before);
        // evolve refuses at the start too, before it records a mutation.
        let e = crate::mutate::evolve(
            &repo,
            &mock(&repo),
            &crate::mutate::EvolveOptions {
                generations: 1,
                base_seed: 0,
            },
        );
        assert!(e.unwrap_err().to_string().contains("shadow_warnings"));
        assert_eq!(repo.events().unwrap().len(), before);
        // A clean layout is fine in error mode.
        let (_d2, clean) = fixture(&mock_toml(1.0, ""));
        set_shadow(&clean, "error");
        let mut o = opts("root");
        o.repeats = Some(1);
        assert!(run_version(&clean, &mock(&clean), &o).is_ok());
    }

    #[test]
    fn off_mode_says_nothing() {
        let (_d, repo) = fixture(&mock_toml(1.0, ""));
        risky(&repo);
        set_shadow(&repo, "off");
        let mut o = opts("root");
        o.repeats = Some(1);
        let report = run_version(&repo, &mock(&repo), &o).unwrap();
        assert!(report.warnings.is_empty());
    }
}
