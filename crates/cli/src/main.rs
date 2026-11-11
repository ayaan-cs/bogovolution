//! `rules-evolve` command line tool.
//!
//! Every command accepts `--json`. See the README for the JSON shapes.

use anyhow::Result;
use clap::{Parser, Subcommand};
use rules_evolve_core::bench::{propose, run_version, verify_child, RunOptions};
use rules_evolve_core::event::{Event, EventKind, Payload};
use rules_evolve_core::init::{init, InitOptions};
use rules_evolve_core::model::build_provider;
use rules_evolve_core::repo::{FullVerify, Repo, StatusReport};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::ExitCode;

/// Version of the JSON output format. Bumped only on breaking changes.
const SCHEMA_VERSION: u32 = 1;

#[derive(Parser)]
#[command(
    name = "rules-evolve",
    version,
    about = "Evolve a repo's agent rules file, one small measured change at a time",
    long_about = "Evolve a repo's agent rules file (AGENTS.md, CLAUDE.md, .cursor/rules).\n\
                  Every attempt is recorded in an append-only, hash-chained log.\n\
                  Phase 1 only sets up the project and reads the log. It makes no model calls."
)]
struct Cli {
    /// Print machine readable JSON instead of text
    #[arg(long, global = true)]
    json: bool,

    /// Repo root to work in
    #[arg(short = 'C', long = "dir", global = true, default_value = ".")]
    dir: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Set up .rules-evolve/ in the repo: config, task template, and the log
    Init {
        /// Rules file to evolve. Detected from AGENTS.md, CLAUDE.md, .cursor/rules if not given
        #[arg(long)]
        rules_file: Option<PathBuf>,
        /// Name of the model that will be used. Only recorded, never called
        #[arg(long)]
        model: String,
    },
    /// Show a summary of the lineage tree and whether the log is intact
    Status,
    /// List log events in order
    Log {
        /// Show only the newest N events
        #[arg(long)]
        limit: Option<usize>,
        /// Show only one kind: init, mutation_proposed, mutation_refused, run_recorded, verdict
        #[arg(long)]
        kind: Option<String>,
    },
    /// Check the hash chain and stored files for tampering or cut off data
    VerifyLog,
    /// Add a child rules version from a file. Temporary stand in for the
    /// model driven `mutate` that comes in a later phase
    Propose {
        /// File holding the full new rules text
        #[arg(long)]
        file: PathBuf,
        /// Plain English reason this change should help
        #[arg(long)]
        hypothesis: String,
        /// Version to change: root, current, or a hash prefix
        #[arg(long, default_value = "current")]
        parent: String,
    },
    /// Benchmark one rules version: run the task set several times and record each run
    Run {
        /// Version to run: root, current, or a hash prefix
        #[arg(long, default_value = "current")]
        version: String,
        /// Number of runs. Defaults to `repeats` from config.toml
        #[arg(long)]
        repeats: Option<u32>,
        /// Base seed. Use the same one for parent and child so runs are paired
        #[arg(long, default_value_t = 0)]
        seed: u64,
        /// Model provider. Only `mock` exists so far
        #[arg(long, default_value = "mock")]
        provider: String,
        /// Settings for the mock model
        #[arg(long, default_value = ".rules-evolve/mock.toml")]
        mock_config: PathBuf,
    },
    /// Compare a child to its parent with the fixed rule and record the verdict
    Verify {
        /// The child version: a hash prefix
        #[arg(long)]
        child: String,
    },
}

/// What a command produced.
struct Output {
    command: &'static str,
    data: Value,
    text: String,
    /// 0 fine, 2 the log or its files failed verification.
    exit: u8,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(out) => {
            if cli.json {
                let doc = json!({
                    "schema_version": SCHEMA_VERSION,
                    "ok": true,
                    "command": out.command,
                    "data": out.data,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&doc).expect("json encodes")
                );
            } else {
                print!("{}", out.text);
            }
            ExitCode::from(out.exit)
        }
        Err(err) => {
            if cli.json {
                let doc = json!({
                    "schema_version": SCHEMA_VERSION,
                    "ok": false,
                    "error": { "message": format!("{err:#}") },
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&doc).expect("json encodes")
                );
            } else {
                eprintln!("error: {err:#}");
            }
            ExitCode::from(1)
        }
    }
}

fn run(cli: &Cli) -> Result<Output> {
    match &cli.command {
        Command::Init { rules_file, model } => {
            let report = init(&InitOptions {
                dir: cli.dir.clone(),
                rules_file: rules_file.clone(),
                model: model.clone(),
            })?;
            let mut text = format!(
                "Initialized rules-evolve in {}\n  rules file: {}{}\n  model:      {} (recorded only)\n  root:       {}\n",
                report.repo_root,
                report.rules_file,
                if report.detected { " (detected)" } else { "" },
                report.model,
                report.root_hash,
            );
            for other in &report.other_candidates {
                text.push_str(&format!("  also found: {other} (not used)\n"));
            }
            for note in &report.notes {
                text.push_str(&format!("  note: {note}\n"));
            }
            text.push_str("Files created:\n");
            for f in &report.files_created {
                text.push_str(&format!("  {f}\n"));
            }
            text.push_str(&format!(
                "\ngit: {}\nCommit .rules-evolve/ so the lineage has git history.\n",
                report.gitignore.detail
            ));
            text.push_str(
                "\nReview .rules-evolve/config.toml and edit .rules-evolve/tasks.toml.\n",
            );
            Ok(Output {
                command: "init",
                data: serde_json::to_value(&report)?,
                text,
                exit: 0,
            })
        }
        Command::Status => {
            let repo = Repo::open(&cli.dir)?;
            let status = repo.status()?;
            let exit = if status.valid { 0 } else { 2 };
            Ok(Output {
                command: "status",
                text: status_text(&status),
                data: serde_json::to_value(&status)?,
                exit,
            })
        }
        Command::Log { limit, kind } => {
            let kind = match kind {
                Some(k) => Some(EventKind::parse(k).ok_or_else(|| {
                    anyhow::anyhow!(
                        "unknown kind {k:?}. Use one of: init, mutation_proposed, mutation_refused, run_recorded, verdict"
                    )
                })?),
                None => None,
            };
            let repo = Repo::open(&cli.dir)?;
            let all = repo.events_of(kind)?;
            let total = all.len();
            let shown: Vec<Event> = match limit {
                Some(n) => all.into_iter().skip(total.saturating_sub(*n)).collect(),
                None => all,
            };
            let mut text = String::new();
            for e in &shown {
                text.push_str(&event_line(e));
                text.push('\n');
            }
            text.push_str(&format!("{} of {} events\n", shown.len(), total));
            Ok(Output {
                command: "log",
                data: json!({ "total": total, "shown": shown.len(), "events": shown }),
                text,
                exit: 0,
            })
        }
        Command::Propose {
            file,
            hypothesis,
            parent,
        } => {
            let repo = Repo::open(&cli.dir)?;
            let content = std::fs::read(file)
                .map_err(|e| anyhow::anyhow!("could not read {}: {e}", file.display()))?;
            let cfg = repo.config()?;
            let out = propose(&repo, &cfg, parent, &content, hypothesis)?;
            let refused = out.outcome == "refused";
            let text = if refused {
                format!(
                    "Refused: {}\nRecorded as event #{}.\n",
                    out.reason.clone().unwrap_or_default(),
                    out.event_seq
                )
            } else {
                format!(
                    "Proposed {} from {} ({} bytes changed, limit {}).\nNext: run both versions, then `verify --child {}`.\n",
                    short(out.child.as_deref().unwrap_or("")),
                    short(&out.parent),
                    out.change_bytes,
                    out.max_change_bytes,
                    short(out.child.as_deref().unwrap_or("")),
                )
            };
            Ok(Output {
                command: "propose",
                data: serde_json::to_value(&out)?,
                text,
                exit: if refused { 3 } else { 0 },
            })
        }
        Command::Run {
            version,
            repeats,
            seed,
            provider,
            mock_config,
        } => {
            let repo = Repo::open(&cli.dir)?;
            let mock_path = if mock_config.is_absolute() {
                mock_config.clone()
            } else {
                cli.dir.join(mock_config)
            };
            let model = build_provider(provider, &mock_path)?;
            let report = run_version(
                &repo,
                model.as_ref(),
                &RunOptions {
                    version: version.clone(),
                    repeats: *repeats,
                    base_seed: *seed,
                },
            )?;
            let mut text = format!(
                "Version {} with provider {} (seed {}, token cap {} per run)\n",
                short(&report.version),
                report.provider,
                report.base_seed,
                report.max_tokens_per_run
            );
            for r in &report.runs {
                text.push_str(&format!(
                    "  run {}: {}/{} passed ({:.0}%), {} tokens{}\n",
                    r.run_index,
                    r.passed,
                    r.total,
                    r.pass_rate * 100.0,
                    r.tokens_used,
                    if r.budget_exhausted {
                        " (stopped at the token cap)"
                    } else {
                        ""
                    }
                ));
            }
            if !report.skipped_existing.is_empty() {
                text.push_str(&format!(
                    "  already recorded, not run again: {:?}\n",
                    report.skipped_existing
                ));
            }
            text.push_str(&format!("Total tokens: {}\n", report.total_tokens));
            Ok(Output {
                command: "run",
                data: serde_json::to_value(&report)?,
                text,
                exit: 0,
            })
        }
        Command::Verify { child } => {
            let repo = Repo::open(&cli.dir)?;
            let out = verify_child(&repo, child)?;
            let d = &out.decision;
            let mut text = format!(
                "{} -> {}: {}\n  parent median {:.3}, child median {:.3}, gain {:.1} points\n  paired wins {} of {} (needs {} and a gain of {} points)\n  {}\n",
                short(&out.parent),
                short(&out.child),
                d.verdict.as_str().to_uppercase(),
                d.parent_median,
                d.child_median,
                d.gain_pp,
                d.paired_wins,
                d.repeats,
                out.min_paired_wins,
                out.accept_threshold_pp,
                d.reason
            );
            for p in &d.pairs {
                text.push_str(&format!(
                    "  pair {}: parent {:.2}, child {:.2}{}\n",
                    p.run_index,
                    p.parent_rate,
                    p.child_rate,
                    if p.win { " (child wins)" } else { "" }
                ));
            }
            Ok(Output {
                command: "verify",
                data: serde_json::to_value(&out)?,
                text,
                exit: 0,
            })
        }
        Command::VerifyLog => {
            let repo = Repo::open(&cli.dir)?;
            let v = repo.verify()?;
            let exit = if v.valid { 0 } else { 2 };
            Ok(Output {
                command: "verify-log",
                text: verify_text(&v),
                data: serde_json::to_value(&v)?,
                exit,
            })
        }
    }
}

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(12)]
}

fn event_summary(e: &Event) -> String {
    match Payload::from_event(e) {
        Ok(Payload::Init(p)) => format!(
            "{} model={} root={}",
            p.rules_file,
            p.model,
            short(&p.root_hash)
        ),
        Ok(Payload::MutationProposed(p)) => format!(
            "{} -> {}: {}",
            short(&p.parent),
            short(&p.child),
            p.hypothesis
        ),
        Ok(Payload::MutationRefused(p)) => {
            format!("from {}: refused: {}", short(&p.parent), p.reason)
        }
        Ok(Payload::RunRecorded(p)) => format!(
            "{} run {}: {}/{} passed, {} tokens{}{}",
            short(&p.node),
            p.run_index,
            p.passed,
            p.total,
            p.tokens_used,
            p.seed.map(|s| format!(", seed {s}")).unwrap_or_default(),
            if p.budget_exhausted {
                ", hit token cap"
            } else {
                ""
            }
        ),
        Ok(Payload::Verdict(p)) => format!(
            "{} -> {}: {}",
            short(&p.parent),
            short(&p.child),
            p.verdict.as_str()
        ),
        Err(_) => "(unreadable payload)".to_string(),
    }
}

fn event_line(e: &Event) -> String {
    format!(
        "#{:<4} {}  {:<17} {}",
        e.seq,
        e.timestamp,
        e.kind.as_str(),
        event_summary(e)
    )
}

fn verdict_label(v: Option<rules_evolve_core::event::VerdictKind>) -> &'static str {
    v.map(|v| v.as_str()).unwrap_or("pending")
}

fn status_text(s: &StatusReport) -> String {
    let mut t = String::new();
    t.push_str(&format!(
        "Log: {} ({} events)\n",
        if s.chain.valid { "intact" } else { "BROKEN" },
        s.chain.events
    ));
    for p in s.chain.problems.iter().chain(s.blob_problems.iter()) {
        t.push_str(&format!("  problem [{}]: {}\n", p.code, p.message));
    }
    if let Some(cfg) = &s.config {
        t.push_str(&format!(
            "Settings: repeats={}, accept threshold={} points, min paired wins={}, token cap per run={}\n",
            cfg.evaluation.repeats,
            cfg.evaluation.accept_threshold_pp,
            cfg.evaluation.min_paired_wins,
            cfg.budget.max_tokens_per_run
        ));
    }
    match &s.tree {
        Some(tree) => {
            let sum = tree.summary();
            t.push_str(&format!(
                "Rules file: {}\nModel: {}\nRoot: {}\nCurrent: {}\n",
                tree.rules_file,
                tree.model,
                short(&tree.root),
                short(&tree.current)
            ));
            t.push_str(&format!(
                "Versions: {} (deepest generation {})\nMutations: {} (accepted {}, rejected {}, noise {}, pending {})\nRefused: {}\nRuns recorded: {} ({} tokens)\n",
                sum.nodes,
                sum.max_generation,
                sum.edges,
                sum.accepted,
                sum.rejected,
                sum.noise,
                sum.pending,
                sum.refused,
                sum.runs,
                sum.tokens_used
            ));
            if !tree.edges.is_empty() {
                t.push_str("Edges:\n");
                for e in &tree.edges {
                    t.push_str(&format!(
                        "  {} -> {}  [{}]  {}\n",
                        short(&e.parent),
                        short(&e.child),
                        verdict_label(e.verdict),
                        e.hypothesis
                    ));
                }
            }
        }
        None => t.push_str("Tree: not shown because the log failed verification\n"),
    }
    t
}

fn verify_text(v: &FullVerify) -> String {
    let mut t = String::new();
    if v.valid {
        t.push_str(&format!(
            "OK: {} events, chain intact, stored files match their hashes\n",
            v.chain.events
        ));
        if let Some(h) = &v.chain.head_hash {
            t.push_str(&format!(
                "Head: seq {} hash {}\n",
                v.chain.head_seq.unwrap_or(0),
                h
            ));
        }
    } else {
        t.push_str("FAILED\n");
        for p in v.chain.problems.iter().chain(v.blob_problems.iter()) {
            let at = match (p.seq, p.line) {
                (Some(s), Some(l)) => format!(" (seq {s}, line {l})"),
                (Some(s), None) => format!(" (seq {s})"),
                (None, Some(l)) => format!(" (line {l})"),
                (None, None) => String::new(),
            };
            t.push_str(&format!("  [{}]{}: {}\n", p.code, at, p.message));
        }
    }
    t
}
