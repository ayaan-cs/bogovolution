//! The terminal loop of `watch`: tails the log, redraws as events arrive, and
//! optionally runs a graft command as a child process while showing it.
//!
//! `watch` only reads. It never appends to the log, so a broken chain stays
//! exactly as it was found.

use crate::state::{ChainStatus, ConfigView, RowId, WatchState};
use crate::svg::buffer_to_svg;
use crate::ui::{buffer_to_text, draw, render_to_buffer, Extra};
use crate::view::{Action, ChildStatus, Overlay, ViewState};
use graft_rules_core::event::Event;
use graft_rules_core::export::unified_diff;
use graft_rules_core::repo::Repo;
use ratatui::crossterm::event::{self, Event as TermEvent, KeyEventKind};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Subcommands that `watch --run` may start. They are the ones that write to
/// the log. Read only commands would show nothing new.
pub const RUNNABLE: [&str; 5] = ["evolve", "mutate", "run", "verify", "propose"];

/// How many reads in a row must fail before the chain is shown as broken. A
/// writer in another process appends the event and the head file one after the
/// other, so a single read can land in between.
const BAD_READS_BEFORE_ALARM: u32 = 3;

#[derive(Debug)]
pub struct WatchOptions {
    pub dir: PathBuf,
    /// How often to look for log changes.
    pub interval: Duration,
    /// Arguments of a graft command to run as a child, from `--run`.
    pub run: Option<Vec<String>>,
    /// The graft binary to start for `--run`. Defaults to this program.
    pub exe: Option<PathBuf>,
}

/// What one read of the log found.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub events: Vec<Event>,
    pub chain: ChainStatus,
    pub config: ConfigView,
}

impl Snapshot {
    pub fn state(&self) -> WatchState {
        WatchState::build(&self.events, self.chain.clone(), self.config)
    }
}

/// Read and chain check the log once. Never writes.
pub fn read_snapshot(repo: &Repo) -> Result<Snapshot, String> {
    let (report, events) = repo.log().verify().map_err(|e| e.to_string())?;
    let problems = report
        .problems
        .iter()
        .map(|p| {
            let mut s = p.message.clone();
            if let Some(line) = p.line {
                s.push_str(&format!(" (line {line})"));
            }
            s
        })
        .collect();
    let chain = ChainStatus {
        valid: report.valid,
        events: report.events,
        problems,
    };
    let config = repo
        .config()
        .map(|c| ConfigView {
            repeats: c.evaluation.repeats,
            accept_threshold_pp: c.evaluation.accept_threshold_pp,
            min_paired_wins: c.evaluation.min_paired_wins,
            max_tokens_per_run: c.budget.max_tokens_per_run,
        })
        .unwrap_or_default();
    Ok(Snapshot {
        events,
        chain,
        config,
    })
}

/// Draw the current log once, without a terminal. Returns the buffer so the
/// caller can make text or SVG from it. `select` is `current`, `root`, or a
/// hash prefix; with `None` the newest row is selected.
pub fn snapshot_buffer(
    dir: &Path,
    width: u16,
    height: u16,
    select: Option<&str>,
) -> Result<ratatui::buffer::Buffer, String> {
    let repo = Repo::open(dir).map_err(|e| e.to_string())?;
    let snap = read_snapshot(&repo)?;
    let state = snap.state();
    let mut view = ViewState::default();
    view.sync(&state);
    if let Some(sel) = select {
        let row = match sel {
            "current" => state.current_row(),
            "root" => state.rows.first(),
            prefix => state
                .rows
                .iter()
                .find(|r| matches!(&r.id, RowId::Version(h) if h.starts_with(prefix))),
        }
        .ok_or_else(|| format!("no version matches {sel:?}"))?;
        view.selected = Some(row.id.clone());
        view.follow = false;
    }
    Ok(render_to_buffer(
        &state,
        &mut view,
        &Extra::default(),
        width,
        height,
    ))
}

pub fn snapshot_text(
    dir: &Path,
    width: u16,
    height: u16,
    select: Option<&str>,
) -> Result<String, String> {
    snapshot_buffer(dir, width, height, select).map(|b| buffer_to_text(&b))
}

pub fn snapshot_svg(
    dir: &Path,
    width: u16,
    height: u16,
    select: Option<&str>,
) -> Result<String, String> {
    snapshot_buffer(dir, width, height, select).map(|b| buffer_to_svg(&b))
}

/// Split a command line into words. Understands single and double quotes.
pub fn split_words(input: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut quote: Option<char> = None;
    for c in input.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                has = true;
            }
            None if c.is_whitespace() => {
                if has || !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            None => cur.push(c),
        }
    }
    if quote.is_some() {
        return Err("the --run text has a quote that is never closed".into());
    }
    if has || !cur.is_empty() {
        words.push(cur);
    }
    Ok(words)
}

/// Turn the `--run` text into arguments for the child. An optional leading
/// `graft` is dropped. Only commands that write the log are allowed,
/// and `-C` is not, because the child always works on the watched folder.
pub fn parse_run(text: &str) -> Result<Vec<String>, String> {
    let mut words = split_words(text)?;
    if words.first().is_some_and(|w| w == "graft") {
        words.remove(0);
    }
    let Some(first) = words.first() else {
        return Err("--run needs a command, for example \"evolve --generations 3\"".into());
    };
    if !RUNNABLE.contains(&first.as_str()) {
        return Err(format!(
            "--run can start one of: {}. Got {first:?}",
            RUNNABLE.join(", ")
        ));
    }
    if words
        .iter()
        .any(|w| w == "-C" || w == "--dir" || w.starts_with("-C="))
    {
        return Err("do not pass -C inside --run. The command works on the watched folder".into());
    }
    Ok(words)
}

struct ChildRun {
    child: Child,
    label: String,
    stderr: tempfile::NamedTempFile,
}

fn start_child(exe: &Path, dir: &Path, args: &[String]) -> Result<ChildRun, String> {
    let stderr = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    let err_handle = stderr.reopen().map_err(|e| e.to_string())?;
    let child = Command::new(exe)
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(err_handle))
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", exe.display()))?;
    Ok(ChildRun {
        child,
        label: short_label(args),
        stderr,
    })
}

/// The command for the status bar: the first words, cut at 36 characters.
pub fn short_label(args: &[String]) -> String {
    let full = args.join(" ");
    if full.chars().count() <= 36 {
        full
    } else {
        let cut: String = full.chars().take(33).collect();
        format!("{cut}...")
    }
}

fn last_stderr_line(file: &tempfile::NamedTempFile) -> String {
    let mut text = String::new();
    if let Ok(mut f) = file.reopen() {
        let _ = f.seek(SeekFrom::Start(0));
        let _ = f.read_to_string(&mut text);
    }
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string()
}

#[derive(PartialEq, Clone, Copy)]
struct Stamp {
    log_len: u64,
    log_mtime: Option<std::time::SystemTime>,
    head_len: u64,
    head_mtime: Option<std::time::SystemTime>,
}

fn stamp(dir: &Path) -> Stamp {
    let root = graft_rules_core::store::Store::for_repo(dir)
        .root()
        .to_path_buf();
    let meta = |name: &str| std::fs::metadata(root.join(name)).ok();
    let log = meta("log.jsonl");
    let head = meta("HEAD");
    Stamp {
        log_len: log.as_ref().map_or(0, |m| m.len()),
        log_mtime: log.and_then(|m| m.modified().ok()),
        head_len: head.as_ref().map_or(0, |m| m.len()),
        head_mtime: head.and_then(|m| m.modified().ok()),
    }
}

/// Diff of a version against its parent, as lines for the overlay.
fn load_diff(repo: &Repo, state: &WatchState, id: &RowId) -> Vec<String> {
    let Some(d) = state.details(id) else {
        return Vec::new();
    };
    let (Some(hash), Some(parent)) = (d.hash, d.parent_hash) else {
        return Vec::new();
    };
    let text = |h: &str| {
        repo.store()
            .get_blob(h)
            .map(|b| String::from_utf8_lossy(&b).to_string())
    };
    match (text(&parent), text(&hash)) {
        (Ok(old), Ok(new)) => unified_diff(&state.rules_file, &old, &new)
            .lines()
            .map(str::to_string)
            .collect(),
        _ => vec!["The stored text for this version could not be read.".to_string()],
    }
}

/// Run the interactive view until the user quits.
pub fn run(opts: &WatchOptions) -> Result<(), String> {
    let repo = Repo::open(&opts.dir).map_err(|e| e.to_string())?;
    let first = read_snapshot(&repo)?;
    let mut state = first.state();
    let mut view = ViewState::default();
    view.sync(&state);
    let mut extra = Extra::default();
    let mut last_stamp = stamp(&opts.dir);
    let mut bad_reads: u32 = 0;
    let mut last_check = Instant::now();

    let mut child: Option<ChildRun> = None;
    if let Some(args) = &opts.run {
        let exe = match &opts.exe {
            Some(p) => p.clone(),
            None => std::env::current_exe().map_err(|e| e.to_string())?,
        };
        let run = start_child(&exe, &opts.dir, args)?;
        extra.child = ChildStatus::Running(run.label.clone());
        child = Some(run);
    }

    let mut terminal = ratatui::init();
    let result = event_loop(
        &mut terminal,
        &repo,
        opts,
        &mut state,
        &mut view,
        &mut extra,
        &mut last_stamp,
        &mut bad_reads,
        &mut last_check,
        &mut child,
    );
    ratatui::restore();
    if let Some(mut c) = child {
        if c.child.try_wait().ok().flatten().is_none() {
            let _ = c.child.kill();
            let _ = c.child.wait();
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    repo: &Repo,
    opts: &WatchOptions,
    state: &mut WatchState,
    view: &mut ViewState,
    extra: &mut Extra,
    last_stamp: &mut Stamp,
    bad_reads: &mut u32,
    last_check: &mut Instant,
    child: &mut Option<ChildRun>,
) -> Result<(), String> {
    loop {
        terminal
            .draw(|f| draw(f, state, view, extra))
            .map_err(|e| e.to_string())?;

        if event::poll(Duration::from_millis(100)).map_err(|e| e.to_string())? {
            if let TermEvent::Key(key) = event::read().map_err(|e| e.to_string())? {
                if key.kind == KeyEventKind::Press {
                    match view.handle_key(state, key, extra.child.is_running()) {
                        Action::Quit => return Ok(()),
                        Action::LoadDiff(id) => {
                            let diff = load_diff(repo, state, &id);
                            if let Some(Overlay { diff: slot, .. }) = view.overlay.as_mut() {
                                *slot = diff;
                            }
                        }
                        Action::None => {}
                    }
                }
            }
        }

        let child_done = poll_child(child, extra);
        if child_done {
            view.quit_armed = false;
        }
        if last_check.elapsed() >= opts.interval || child_done {
            *last_check = Instant::now();
            let now = stamp(&opts.dir);
            if now != *last_stamp || *bad_reads > 0 || child_done {
                *last_stamp = now;
                if let Ok(snap) = read_snapshot(repo) {
                    if snap.chain.valid {
                        *bad_reads = 0;
                        *state = snap.state();
                        view.sync(state);
                    } else {
                        *bad_reads += 1;
                        if *bad_reads >= BAD_READS_BEFORE_ALARM {
                            *state = snap.state();
                            view.sync(state);
                        }
                    }
                }
            }
        }
    }
}

/// Update the child status. Returns true the moment the child has exited.
fn poll_child(child: &mut Option<ChildRun>, extra: &mut Extra) -> bool {
    let Some(c) = child.as_mut() else {
        return false;
    };
    match c.child.try_wait() {
        Ok(None) => false,
        Ok(Some(status)) => {
            extra.child = if status.success() {
                ChildStatus::Exited {
                    ok: true,
                    text: format!("{} finished", c.label),
                }
            } else {
                let code = status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "a signal".to_string());
                let msg = last_stderr_line(&c.stderr);
                ChildStatus::Exited {
                    ok: false,
                    text: format!("{} failed (exit {code}) {msg}", c.label),
                }
            };
            *child = None;
            true
        }
        Err(e) => {
            extra.child = ChildStatus::Exited {
                ok: false,
                text: format!("could not check the child process: {e}"),
            };
            *child = None;
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_words_with_quotes() {
        assert_eq!(
            split_words("evolve --generations 3").unwrap(),
            ["evolve", "--generations", "3"]
        );
        assert_eq!(
            split_words("  a   'b c'  \"d e\" ''  ").unwrap(),
            ["a", "b c", "d e", ""]
        );
        assert!(split_words("evolve 'oops").is_err());
        assert!(split_words("   ").unwrap().is_empty());
    }

    #[test]
    fn run_text_is_checked() {
        assert_eq!(
            parse_run("evolve --generations 3 --seed 7").unwrap(),
            ["evolve", "--generations", "3", "--seed", "7"]
        );
        assert_eq!(
            parse_run("graft evolve --generations 1").unwrap()[0],
            "evolve"
        );
        assert!(parse_run("").unwrap_err().contains("needs a command"));
        assert!(parse_run("status").unwrap_err().contains("--run can start"));
        assert!(parse_run("watch").is_err());
        assert!(parse_run("evolve -C /tmp").unwrap_err().contains("-C"));
        assert!(parse_run("rm -rf x").is_err());
    }

    #[test]
    fn long_commands_are_cut_for_the_status_bar() {
        let short = vec![
            "evolve".to_string(),
            "--generations".to_string(),
            "3".to_string(),
        ];
        assert_eq!(short_label(&short), "evolve --generations 3");
        let long: Vec<String> = parse_run(
            "evolve --generations 10 --seed 7 --provider replay --replay-file /a/very/long/path.jsonl",
        )
        .unwrap();
        let label = short_label(&long);
        assert_eq!(label.chars().count(), 36);
        assert!(label.ends_with("..."));
    }

    fn project() -> (tempfile::TempDir, Repo) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "# Rules\nBe careful.\n").unwrap();
        graft_rules_core::init::init(&graft_rules_core::init::InitOptions {
            dir: dir.path().to_path_buf(),
            rules_file: None,
            model: "demo-model".into(),
        })
        .unwrap();
        let repo = Repo::open(dir.path()).unwrap();
        (dir, repo)
    }

    #[test]
    fn a_fresh_project_shows_the_root() {
        let (dir, repo) = project();
        let snap = read_snapshot(&repo).unwrap();
        assert!(snap.chain.valid);
        assert_eq!(snap.chain.events, 1);
        assert_eq!(snap.config.repeats, 5);
        let text = snapshot_text(dir.path(), 110, 32, None).unwrap();
        assert!(text.contains("starting rules"));
        assert!(text.contains("chain OK"));
    }

    #[test]
    fn new_events_show_up_on_the_next_read() {
        use graft_rules_core::event::{MutationProposedPayload, Payload};
        let (dir, repo) = project();
        let before = read_snapshot(&repo).unwrap();
        let hash = match &before.state().rows[0].id {
            RowId::Version(h) => h.clone(),
            RowId::Refused(_) => unreachable!(),
        };
        let child = repo
            .store()
            .put_blob(b"# Rules\nBe careful.\nRun tests.\n")
            .unwrap();
        repo.append(&Payload::MutationProposed(MutationProposedPayload {
            parent: hash,
            child,
            hypothesis: "Run the tests.".into(),
            ..Default::default()
        }))
        .unwrap();
        let after = read_snapshot(&repo).unwrap();
        assert_eq!(after.chain.events, 2);
        let state = after.state();
        assert_eq!(state.rows.len(), 2);
        assert_eq!(state.pending, 1);
        let text = snapshot_text(dir.path(), 110, 32, None).unwrap();
        assert!(text.contains("Run the tests."));
    }

    #[test]
    fn an_edited_log_is_reported_and_never_repaired() {
        let (dir, repo) = project();
        let log = dir.path().join(".graft").join("log.jsonl");
        let original = std::fs::read_to_string(&log).unwrap();
        let edited = original.replacen("demo-model", "evil-model", 1);
        std::fs::write(&log, &edited).unwrap();
        let snap = read_snapshot(&repo).unwrap();
        assert!(!snap.chain.valid);
        assert!(!snap.chain.problems.is_empty());
        let text = snapshot_text(dir.path(), 110, 32, None).unwrap();
        assert!(text.contains("CHAIN FAILED VERIFICATION"));
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            edited,
            "watch only reads"
        );
    }

    #[test]
    fn snapshot_select_finds_versions() {
        let (dir, _repo) = project();
        assert!(snapshot_text(dir.path(), 110, 32, Some("current")).is_ok());
        assert!(snapshot_text(dir.path(), 110, 32, Some("root")).is_ok());
        assert!(snapshot_text(dir.path(), 110, 32, Some("zzzz"))
            .unwrap_err()
            .contains("no version matches"));
    }

    #[test]
    fn a_missing_project_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        assert!(snapshot_text(dir.path(), 100, 30, None)
            .unwrap_err()
            .contains("not a graft project"));
    }
}
