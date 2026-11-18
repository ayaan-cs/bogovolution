//! `rules-evolve` command line tool.
//!
//! Every command accepts `--json`. See the README for the JSON shapes.

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use rules_evolve_core::apply::{apply, ApplyOptions};
use rules_evolve_core::bench::{propose, run_version, verify_child, RunOptions};
use rules_evolve_core::digest::{build_digest, render_markdown};
use rules_evolve_core::event::{Event, EventKind, Payload};
use rules_evolve_core::export::{build_export, to_json as export_json};
use rules_evolve_core::init::{init, InitOptions};
use rules_evolve_core::isolation::Isolation;
use rules_evolve_core::model::{build_provider, Model, ProviderSpec};
use rules_evolve_core::mutate::{evolve, mutate, EvolveOptions, MutateOptions};
use rules_evolve_core::repo::{FullVerify, Repo, StatusReport};
use rules_evolve_core::shadow::{self, ShadowWarning};
use rules_evolve_core::tree::Tree;
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

/// Which model provider to use and where its files are.
#[derive(Args, Clone)]
struct ProviderArgs {
    /// Model provider: `mock`, `replay` or `local` (a model server on this
    /// machine, localhost only) are free. `anthropic` and `openai` are PAID on
    /// your own key and need --allow-spend and --api-model
    #[arg(long, default_value = "mock")]
    provider: String,
    /// Confirm that `--provider anthropic` or `--provider openai` may spend
    /// money on your own key. Every task attempt and mutation is a billed call
    #[arg(long)]
    allow_spend: bool,
    /// Model id for a paid provider, for example the one your account uses.
    /// There is no default
    #[arg(long, value_name = "MODEL")]
    api_model: Option<String>,
    /// Base URL for a paid provider. Defaults to https://api.anthropic.com or
    /// https://api.openai.com/v1. Any OpenAI compatible https URL works with
    /// `openai`. Plain http only for localhost
    #[arg(long, value_name = "URL")]
    api_url: Option<String>,
    /// Name of the environment variable that holds the key (default
    /// ANTHROPIC_API_KEY or OPENAI_API_KEY). The key itself is never a flag
    #[arg(long, value_name = "VAR")]
    api_key_env: Option<String>,
    /// Sampling temperature for a paid provider (0 to 2, Anthropic caps at 1)
    #[arg(long, default_value_t = 0.7, value_name = "T")]
    api_temperature: f64,
    /// Send no temperature to the paid provider (some newer models refuse it)
    #[arg(long)]
    api_no_temperature: bool,
    /// Most tokens a paid model may write per call
    #[arg(long, default_value_t = 1024, value_name = "N")]
    api_max_new_tokens: u64,
    /// Time limit for one paid call, in seconds
    #[arg(long, default_value_t = 300, value_name = "SECS")]
    api_timeout: u64,
    /// Append every raw paid model reply to this JSONL file (never the key)
    #[arg(long, value_name = "FILE")]
    api_transcript: Option<PathBuf>,
    /// Base URL of the local model server. Only for `--provider local`. The host
    /// must be localhost, 127.0.0.1 or [::1]
    #[arg(long, default_value = rules_evolve_core::local::DEFAULT_URL, value_name = "URL")]
    local_url: String,
    /// Sampling temperature for `--provider local` (0 to 2)
    #[arg(long, default_value_t = 0.7, value_name = "T")]
    local_temperature: f64,
    /// Most tokens the local model may write per call
    #[arg(long, default_value_t = 1024, value_name = "N")]
    local_max_new_tokens: u64,
    /// Time limit for one local model call, in seconds
    #[arg(long, default_value_t = 600, value_name = "SECS")]
    local_timeout: u64,
    /// Append every raw local model reply to this JSONL file (for debugging)
    #[arg(long, value_name = "FILE")]
    local_transcript: Option<PathBuf>,
    /// Settings for the mock model
    #[arg(long, default_value = ".rules-evolve/mock.toml")]
    mock_config: PathBuf,
    /// Replay file to answer from (needed for `--provider replay`)
    #[arg(long)]
    replay_file: Option<PathBuf>,
    /// Record every model request and response into this replay file
    #[arg(long)]
    record: Option<PathBuf>,
    /// Wait this many milliseconds before every replayed answer, so a replay
    /// is slow enough to watch in `watch`. Only for `--provider replay`
    #[arg(long, default_value_t = 0, value_name = "MS")]
    pace: u64,
    /// How check commands are isolated: auto, bwrap, docker, podman or none.
    /// Overrides `[sandbox] isolation` in config.toml. `none` is unsafe
    #[arg(long, value_name = "BACKEND")]
    isolation: Option<String>,
}

impl ProviderArgs {
    /// Open the repo with the `--isolation` override applied.
    fn open_repo(&self, dir: &std::path::Path) -> Result<Repo> {
        let mut repo = Repo::open(dir)?;
        let over = self
            .isolation
            .as_deref()
            .map(str::parse::<Isolation>)
            .transpose()?;
        repo.set_isolation_override(over);
        Ok(repo)
    }

    fn build(&self, dir: &std::path::Path) -> Result<Box<dyn Model>> {
        let rel = |p: &PathBuf| {
            if p.is_absolute() {
                p.clone()
            } else {
                dir.join(p)
            }
        };
        let paid = matches!(self.provider.as_str(), "anthropic" | "openai");
        if !paid && (self.allow_spend || self.api_model.is_some() || self.api_url.is_some()) {
            return Err(rules_evolve_core::Error::Input(format!(
                "--allow-spend, --api-model and --api-url only apply to --provider anthropic or openai, not {:?}",
                self.provider
            ))
            .into());
        }
        if self.provider == "local" || paid {
            for (flag, p) in [
                ("--record", self.record.as_ref()),
                ("--local-transcript", self.local_transcript.as_ref()),
                ("--api-transcript", self.api_transcript.as_ref()),
            ] {
                if let Some(p) = p {
                    if model_can_see(dir, &rel(p)) {
                        return Err(rules_evolve_core::Error::Input(format!(
                            "{flag} {} is inside the repo, where the model would read it as a project file on the next task. Put it outside the repo or under .rules-evolve/",
                            p.display()
                        ))
                        .into());
                    }
                }
            }
        }
        Ok(build_provider(&ProviderSpec {
            name: self.provider.clone(),
            mock_config: rel(&self.mock_config),
            replay_file: self.replay_file.as_ref().map(rel),
            record: self.record.as_ref().map(rel),
            pace_ms: self.pace,
            local: rules_evolve_core::local::LocalSettings {
                url: self.local_url.clone(),
                temperature: self.local_temperature,
                max_new_tokens: self.local_max_new_tokens,
                timeout_secs: self.local_timeout,
                transcript: self.local_transcript.as_ref().map(rel),
            },
            remote: rules_evolve_core::remote::RemoteSettings {
                url: self.api_url.clone().unwrap_or_default(),
                model: self.api_model.clone().unwrap_or_default(),
                key_env: self.api_key_env.clone().unwrap_or_default(),
                allow_spend: self.allow_spend,
                temperature: self.api_temperature,
                omit_temperature: self.api_no_temperature,
                max_new_tokens: self.api_max_new_tokens,
                timeout_secs: self.api_timeout,
                transcript: self.api_transcript.as_ref().map(rel),
            },
        })?)
    }
}

/// True when `path` lands in the repo copy the model is shown: inside `dir`
/// and not under a hidden (dot) folder such as `.rules-evolve`.
fn model_can_see(dir: &std::path::Path, path: &std::path::Path) -> bool {
    use std::path::{Component, PathBuf};
    let Ok(root) = dir.canonicalize() else {
        return false;
    };
    // resolve `..` and `.` by hand, then any symlinks in the part that exists
    let mut norm = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                norm.pop();
            }
            Component::CurDir => {}
            other => norm.push(other),
        }
    }
    let mut existing = norm.clone();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    while !existing.exists() {
        match existing.file_name() {
            Some(n) => rest.push(n.to_os_string()),
            None => break,
        }
        existing.pop();
    }
    let mut full = existing.canonicalize().unwrap_or(existing);
    for n in rest.into_iter().rev() {
        full.push(n);
    }
    let Ok(inside) = full.strip_prefix(&root) else {
        return false;
    };
    !inside
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
}

/// Add replay counts to a command's JSON data.
fn with_counters(mut data: Value, model: &dyn Model) -> Result<Value> {
    if let Value::Object(map) = &mut data {
        map.insert(
            "provider_stats".into(),
            serde_json::to_value(model.counters())?,
        );
    }
    Ok(data)
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
    /// Live terminal view of the lineage. Follows the log as another process
    /// runs `evolve`. Read only
    #[cfg(feature = "watch")]
    Watch {
        /// Run a rules-evolve command as a child process and show it live, for
        /// example "evolve --generations 3 --provider replay --replay-file r.jsonl"
        #[arg(long, value_name = "COMMAND")]
        run: Option<String>,
        /// How often to look for log changes, in milliseconds
        #[arg(long, default_value_t = 200, value_name = "MS")]
        interval: u64,
        /// Print one text picture of the view and exit. Needs no terminal
        #[arg(long)]
        snapshot: bool,
        /// With --snapshot, write a colored SVG picture to this file instead
        #[arg(long, value_name = "PATH", requires = "snapshot")]
        svg: Option<PathBuf>,
        /// With --snapshot, picture width in columns
        #[arg(long, default_value_t = 110, requires = "snapshot")]
        width: u16,
        /// With --snapshot, picture height in rows
        #[arg(long, default_value_t = 32, requires = "snapshot")]
        height: u16,
        /// With --snapshot, which version to select: current, root, or a hash prefix
        #[arg(long, value_name = "VERSION", requires = "snapshot")]
        select: Option<String>,
    },
    /// Write a version into the repo's rules file. A dry run (diff only) unless
    /// --yes is given. Only accepted versions, unless --force. No model is called
    Apply {
        /// current (the default), root, a hash, or a unique hash prefix
        #[arg(long, default_value = "current")]
        version: String,
        /// Write the file. Without it nothing is written
        #[arg(long)]
        yes: bool,
        /// Allow a version that was not accepted (root, pending, rejected, noise, overfit)
        #[arg(long)]
        force: bool,
    },
    /// Plain English brief of the lineage: what worked, what failed, what is
    /// noise, what is pending, and directions not tried yet. No model is called
    Digest,
    /// Write the whole lineage as one JSON file (schema/export.v1.json) for the
    /// static viewer in viewer/
    Export {
        /// Where to write the JSON file. Use - for standard output
        #[arg(long)]
        out: PathBuf,
    },
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
        #[command(flatten)]
        provider: ProviderArgs,
    },
    /// Ask the model for a small rules change plus a hypothesis, and record it
    Mutate {
        /// Version to change: root, current, or a hash prefix
        #[arg(long, default_value = "current")]
        parent: String,
        /// Seed for the model request
        #[arg(long, default_value_t = 0)]
        seed: u64,
        #[command(flatten)]
        provider: ProviderArgs,
    },
    /// Loop mutate, run, verify for N generations. `current` moves only on accepted
    Evolve {
        /// Number of generations (1 to 100)
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=100))]
        generations: u32,
        /// Base seed for runs and mutations. Same seed, same lineage
        #[arg(long, default_value_t = 0)]
        seed: u64,
        /// --generations is the total for the project: generations already in
        /// the log (judged children and refused attempts) count, so the same
        /// command run again after a cut off session stops at the same total
        #[arg(long)]
        total: bool,
        #[command(flatten)]
        provider: ProviderArgs,
    },
    /// Show which isolation backends work on this machine and which one `auto` picks
    Isolation,
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
            // The tasks.toml that init just wrote is the template, so this is
            // empty in practice. It is here so the shape matches the other
            // commands, and so a template with hidden entries would be flagged.
            let (_, warnings) = shadow::scan_repo(&Repo::open(&cli.dir)?)?;
            if !warnings.is_empty() {
                text.push('\n');
                text.push_str(&warnings_text(&warnings));
            }
            text.push_str(
                "\nReview .rules-evolve/config.toml and edit .rules-evolve/tasks.toml.\n",
            );
            let mut data = serde_json::to_value(&report)?;
            if let Value::Object(map) = &mut data {
                map.insert("warnings".into(), serde_json::to_value(&warnings)?);
            }
            Ok(Output {
                command: "init",
                data,
                text,
                exit: 0,
            })
        }
        Command::Isolation => {
            let repo = Repo::open(&cli.dir)?;
            let settings = repo.sandbox_settings()?;
            settings.validate()?;
            let (backends, auto) = rules_evolve_core::isolation::survey(&settings);
            let (shadow_mode, warnings) = shadow::scan_repo(&repo)?;
            let mut text = format!("Configured isolation: {}\n", settings.isolation.as_str());
            for b in &backends {
                match &b.problem {
                    None => text.push_str(&format!("  {}: works\n", b.backend.as_str())),
                    Some(p) => text.push_str(&format!(
                        "  {}: does not work here ({})\n",
                        b.backend.as_str(),
                        p
                    )),
                }
            }
            text.push_str(&match auto {
                Some(b) => format!("auto would use: {}\n", b.as_str()),
                None => format!(
                    "auto would use: {} (a real provider would be refused)\n",
                    rules_evolve_core::isolation::UNSAFE_LABEL
                ),
            });
            text.push_str(&format!("Shadow warnings: {}\n", shadow_mode.as_str()));
            if warnings.is_empty() {
                text.push_str("  none found\n");
            } else {
                text.push('\n');
                text.push_str(&warnings_text(&warnings));
            }
            Ok(Output {
                command: "isolation",
                data: json!({
                    "configured": settings.isolation.as_str(),
                    "image": settings.image,
                    "backends": backends,
                    "auto": auto.map(|b| b.as_str()).unwrap_or("none"),
                    "shadow_warnings": shadow_mode.as_str(),
                    "warnings": warnings,
                }),
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
        } => {
            let repo = provider.open_repo(&cli.dir)?;
            let model = provider.build(&cli.dir)?;
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
            text.push_str(&warnings_text(&report.warnings));
            for r in &report.runs {
                let held = match (r.held_out_passed, r.held_out_total) {
                    (Some(hp), Some(ht)) => format!(" train, {hp}/{ht} held out"),
                    _ => String::new(),
                };
                text.push_str(&format!(
                    "  run {}: {}/{} passed ({:.0}%){held}, {} tokens{}\n",
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
            if let Some(note) = &report.recovered_log {
                text.push_str(&format!("Log repaired: {note}\n"));
            }
            if let Some(note) = &report.stopped_at_token_cap {
                text.push_str(&format!("STOPPED AT THE TOKEN CAP: {note}\n"));
            }
            text.push_str(&format!(
                "Isolation: {}\n",
                label_isolation(&report.isolation)
            ));
            text.push_str(&format!("Total tokens: {}\n", report.total_tokens));
            text.push_str(&counters_text(model.as_ref(), provider));
            Ok(Output {
                command: "run",
                data: with_counters(serde_json::to_value(&report)?, model.as_ref())?,
                text,
                exit: 0,
            })
        }
        Command::Mutate {
            parent,
            seed,
            provider,
        } => {
            let repo = provider.open_repo(&cli.dir)?;
            let model = provider.build(&cli.dir)?;
            let out = mutate(
                &repo,
                model.as_ref(),
                &MutateOptions {
                    parent: parent.clone(),
                    seed: *seed,
                },
            )?;
            let refused = out.result.outcome == "refused";
            let mut text = if refused {
                format!(
                    "Refused: {}\nRecorded as event #{}.\n",
                    out.result.reason.clone().unwrap_or_default(),
                    out.result.event_seq
                )
            } else {
                let child = out.result.child.clone().unwrap_or_default();
                format!(
                    "Proposed {} from {} ({} bytes changed, limit {}).\n  hypothesis: {}\nNext: run both versions, then `verify --child {}`.\n",
                    short(&child),
                    short(&out.result.parent),
                    out.result.change_bytes,
                    out.result.max_change_bytes,
                    out.result.hypothesis,
                    short(&child),
                )
            };
            text.push_str(&format!(
                "Provider {}, seed {}, {} tokens.\n",
                out.provider, out.seed, out.tokens_used
            ));
            text.push_str(&counters_text(model.as_ref(), provider));
            Ok(Output {
                command: "mutate",
                data: with_counters(serde_json::to_value(&out)?, model.as_ref())?,
                text,
                exit: if refused { 3 } else { 0 },
            })
        }
        Command::Evolve {
            generations,
            seed,
            total,
            provider,
        } => {
            let repo = provider.open_repo(&cli.dir)?;
            let model = provider.build(&cli.dir)?;
            let report = evolve(
                &repo,
                model.as_ref(),
                &EvolveOptions {
                    generations: *generations,
                    base_seed: *seed,
                    total: *total,
                },
            )?;
            let mut text = format!(
                "Evolved {} generations with provider {} (seed {})\n",
                report.generations.len(),
                report.provider,
                report.base_seed
            );
            text = format!("{}{}", warnings_text(&report.warnings), text);
            if *total {
                text.push_str(&format!(
                    "Total {}: {} generations were already in the log.\n",
                    generations, report.generations_already_done
                ));
            }
            if let Some(note) = &report.recovered_log {
                text.push_str(&format!("Log repaired: {note}\n"));
            }
            for g in &report.generations {
                let what = match &g.verdict {
                    Some(v) => format!(
                        "{} (gain {:.1} points, {} of {} wins{})",
                        v.decision.verdict.as_str().to_uppercase(),
                        v.decision.gain_pp,
                        v.decision.paired_wins,
                        v.decision.repeats,
                        match &v.decision.held_out {
                            Some(h) => format!(
                                "; held out gain {:.1} points, {} of {} wins",
                                h.gain_pp, h.paired_wins, v.decision.repeats
                            ),
                            None => String::new(),
                        }
                    ),
                    None => format!(
                        "REFUSED ({})",
                        g.mutation.result.reason.clone().unwrap_or_default()
                    ),
                };
                for r in &g.refused_attempts {
                    text.push_str(&format!(
                        "  gen {}: refused attempt ({r}), asked again with a new seed\n",
                        g.generation
                    ));
                }
                text.push_str(&format!(
                    "  gen {}: {}{}\n    {}\n    current: {}\n",
                    g.generation,
                    if g.resumed {
                        "finished an unfinished child from an earlier session: "
                    } else {
                        ""
                    },
                    what,
                    if g.mutation.result.hypothesis.is_empty() {
                        "(no hypothesis)"
                    } else {
                        &g.mutation.result.hypothesis
                    },
                    short(&g.current)
                ));
            }
            if let Some(iso) = &report.isolation {
                text.push_str(&format!("Isolation: {}\n", label_isolation(iso)));
            }
            text.push_str(&format!(
                "Accepted {}, rejected {}, noise {}, {}refused {}. Total tokens: {}\n",
                report.accepted,
                report.rejected,
                report.noise,
                if report.overfit > 0 {
                    format!("overfit {}, ", report.overfit)
                } else {
                    String::new()
                },
                report.refused,
                report.total_tokens
            ));
            text.push_str(&format!(
                "Current: {} (was {})\n",
                short(&report.end),
                short(&report.start)
            ));
            text.push_str(&counters_text(model.as_ref(), provider));
            Ok(Output {
                command: "evolve",
                data: with_counters(serde_json::to_value(&report)?, model.as_ref())?,
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
            if let Some(h) = &d.held_out {
                text.push_str(&format!(
                    "  held out: parent median {:.3}, child median {:.3}, gain {:.1} points, paired wins {} of {} (needs {} and a gain of {} points)\n",
                    h.parent_median,
                    h.child_median,
                    h.gain_pp,
                    h.paired_wins,
                    d.repeats,
                    out.held_out_min_paired_wins.unwrap_or(0),
                    out.held_out_accept_threshold_pp.unwrap_or(0.0)
                ));
            }
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
        #[cfg(feature = "watch")]
        Command::Watch {
            run,
            interval,
            snapshot,
            svg,
            width,
            height,
            select,
        } => {
            use rules_evolve_watch::app;
            if *snapshot {
                if run.is_some() {
                    anyhow::bail!("--snapshot and --run cannot be used together");
                }
                let select = select.as_deref();
                if let Some(path) = svg {
                    let doc = app::snapshot_svg(&cli.dir, *width, *height, select)
                        .map_err(anyhow::Error::msg)?;
                    std::fs::write(path, &doc)?;
                    return Ok(Output {
                        command: "watch",
                        data: json!({ "svg": path.display().to_string(), "bytes": doc.len() }),
                        text: format!("Wrote {} ({} bytes).\n", path.display(), doc.len()),
                        exit: 0,
                    });
                }
                let text = app::snapshot_text(&cli.dir, *width, *height, select)
                    .map_err(anyhow::Error::msg)?;
                return Ok(Output {
                    command: "watch",
                    data: json!({ "width": width, "height": height, "text": text }),
                    text,
                    exit: 0,
                });
            }
            if cli.json {
                anyhow::bail!("watch is interactive and has no --json output. Use --snapshot --json for one picture");
            }
            Repo::open(&cli.dir)?;
            let child = match run {
                Some(text) => Some(app::parse_run(text).map_err(anyhow::Error::msg)?),
                None => None,
            };
            if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
                anyhow::bail!(
                    "watch needs a terminal. Use --snapshot to print one picture instead"
                );
            }
            app::run(&app::WatchOptions {
                dir: cli.dir.clone(),
                interval: std::time::Duration::from_millis((*interval).max(50)),
                run: child,
                exe: None,
            })
            .map_err(anyhow::Error::msg)?;
            Ok(Output {
                command: "watch",
                data: json!({}),
                text: String::new(),
                exit: 0,
            })
        }
        Command::Apply {
            version,
            yes,
            force,
        } => {
            let repo = Repo::open(&cli.dir)?;
            if !repo.verify()?.valid {
                anyhow::bail!(
                    "the log fails verification, so nothing is applied. Run `verify-log` for details"
                );
            }
            let o = apply(
                &repo,
                &ApplyOptions {
                    version: version.clone(),
                    yes: *yes,
                    force: *force,
                },
            )?;
            let head = format!(
                "Version {} ({}{}) for {}\n",
                short(&o.version),
                o.status,
                if o.forced { ", forced" } else { "" },
                o.rules_file
            );
            let text = match o.outcome.as_str() {
                "unchanged" => format!(
                    "{head}{} already holds this version. Nothing to do.\n",
                    o.rules_file
                ),
                "dry_run" => format!(
                    "{head}{}\nDry run: nothing was written. Run again with --yes to write {}.\n",
                    o.diff, o.rules_file
                ),
                _ => format!(
                    "{head}{}\nWrote {} and recorded it in the log (event {}).\n",
                    o.diff,
                    o.rules_file,
                    o.event_seq.unwrap_or_default()
                ),
            };
            Ok(Output {
                command: "apply",
                data: serde_json::to_value(&o)?,
                text,
                exit: 0,
            })
        }
        Command::Digest => {
            let repo = Repo::open(&cli.dir)?;
            let v = repo.verify()?;
            if !v.valid {
                anyhow::bail!(
                    "the log fails verification, so a digest would not be trustworthy. Run `verify-log` for details"
                );
            }
            let tree = Tree::from_events(&repo.events()?)?;
            let digest = build_digest(&tree);
            Ok(Output {
                command: "digest",
                text: render_markdown(&digest),
                data: serde_json::to_value(&digest)?,
                exit: 0,
            })
        }
        Command::Export { out } => {
            let repo = Repo::open(&cli.dir)?;
            let export = build_export(&repo)?;
            let json = export_json(&export);
            let to_stdout = out.as_os_str() == "-";
            if to_stdout {
                // Text mode prints the bare export. With --json it is the data field.
                return Ok(Output {
                    command: "export",
                    data: serde_json::to_value(&export)?,
                    text: json,
                    exit: 0,
                });
            }
            if let Some(parent) = out.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            std::fs::write(out, &json)?;
            let text = format!(
                "Wrote {} ({} bytes): {} versions, {} edges, {} refused, chain head {}.\nOpen it in the viewer (see README, section Viewer).\n",
                out.display(),
                json.len(),
                export.summary.nodes,
                export.summary.edges,
                export.summary.refused,
                short(&export.chain.head_hash),
            );
            Ok(Output {
                command: "export",
                data: json!({
                    "out": out.display().to_string(),
                    "bytes": json.len(),
                    "format_version": export.version,
                    "nodes": export.summary.nodes,
                    "edges": export.summary.edges,
                    "refused": export.summary.refused,
                    "head_hash": export.chain.head_hash,
                }),
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

fn counters_text(model: &dyn Model, args: &ProviderArgs) -> String {
    let c = model.counters();
    if args.record.is_some() {
        format!("Recorded {} new model calls.\n", c.recorded)
    } else if args.provider == "replay" {
        format!("Served {} answers from the replay file.\n", c.replayed)
    } else {
        String::new()
    }
}

/// Shadow warnings as lines for people, one blank line after the block.
fn warnings_text(warnings: &[ShadowWarning]) -> String {
    if warnings.is_empty() {
        return String::new();
    }
    let mut t = String::new();
    for w in warnings {
        t.push_str(&w.text());
        t.push('\n');
    }
    t.push('\n');
    t
}

/// Backend name for people. `none` always says it is unsafe.
fn label_isolation(name: &str) -> &str {
    if name == "none" {
        rules_evolve_core::isolation::UNSAFE_LABEL
    } else {
        name
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
            "{} run {}: {}/{} passed{}, {} tokens{}{}",
            short(&p.node),
            p.run_index,
            p.passed,
            p.total,
            match (p.held_out_passed, p.held_out_total) {
                (Some(hp), Some(ht)) => format!(" (train), {hp}/{ht} held out"),
                _ => String::new(),
            },
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
        Ok(Payload::Applied(p)) => format!(
            "{} written to {} ({}{})",
            short(&p.version),
            p.rules_file,
            p.status,
            if p.forced { ", forced" } else { "" }
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
                "Versions: {} (deepest generation {})\nMutations: {} (accepted {}, rejected {}, noise {}, {}pending {})\nRefused: {}\nRuns recorded: {} ({} tokens)\n",
                sum.nodes,
                sum.max_generation,
                sum.edges,
                sum.accepted,
                sum.rejected,
                sum.noise,
                if sum.overfit > 0 {
                    format!("overfit {}, ", sum.overfit)
                } else {
                    String::new()
                },
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
