//! The `init` operation: pick the rules file, record the model, write files.

use crate::config::{Config, TASKS_TEMPLATE};
use crate::error::{io_err, Error, Result};
use crate::event::{InitPayload, Payload};
use crate::log::Log;
use crate::store::{Store, DIR_NAME};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Where rules files are looked for, in order.
pub const CANDIDATES: [&str; 3] = ["AGENTS.md", "CLAUDE.md", ".cursor/rules"];

/// Result of looking for a rules file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Detection {
    /// The file that was picked (relative, forward slashes).
    pub chosen: String,
    /// Other rules files that were found but not picked.
    pub others: Vec<String>,
    /// Notes about candidates that could not be used.
    pub notes: Vec<String>,
}

/// Find a rules file in `repo_dir`. The first of `AGENTS.md`, `CLAUDE.md`,
/// `.cursor/rules` that resolves to one file wins.
///
/// `.cursor/rules` may be a file, or a folder. A folder counts only when it
/// holds exactly one file, because Phase 1 evolves one file at a time.
pub fn detect_rules_file(repo_dir: &Path) -> Result<Detection> {
    let mut found: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for cand in CANDIDATES {
        let path = repo_dir.join(cand);
        if path.is_file() {
            found.push(cand.to_string());
        } else if path.is_dir() {
            let mut files: Vec<String> = std::fs::read_dir(&path)
                .map_err(io_err(&path))?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            files.sort();
            match files.len() {
                1 => found.push(format!("{cand}/{}", files[0])),
                0 => notes.push(format!("{cand} is a folder with no files")),
                n => notes.push(format!(
                    "{cand} is a folder with {n} files; pass one with --rules-file"
                )),
            }
        }
    }
    if found.is_empty() {
        let mut msg = format!(
            "no rules file found in {}. Looked for AGENTS.md, CLAUDE.md and .cursor/rules. Pass one with --rules-file.",
            repo_dir.display()
        );
        for n in &notes {
            msg.push_str(&format!(" Note: {n}."));
        }
        return Err(Error::Input(msg));
    }
    let chosen = found.remove(0);
    Ok(Detection {
        chosen,
        others: found,
        notes,
    })
}

#[derive(Debug, Clone)]
pub struct InitOptions {
    /// Repo root.
    pub dir: PathBuf,
    /// Rules file, absolute or relative to `dir`. Detected when `None`.
    pub rules_file: Option<PathBuf>,
    /// Model name. Only recorded.
    pub model: String,
}

/// What `init` did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InitReport {
    pub repo_root: String,
    pub rules_file: String,
    pub detected: bool,
    pub other_candidates: Vec<String>,
    pub notes: Vec<String>,
    pub model: String,
    pub root_hash: String,
    pub defaults_confirmed: bool,
    pub files_created: Vec<String>,
}

fn to_forward_slashes(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Turn a user supplied path into a path relative to the repo root.
fn relativize(repo_dir: &Path, given: &Path) -> Result<String> {
    let abs = if given.is_absolute() {
        given.to_path_buf()
    } else {
        repo_dir.join(given)
    };
    let abs = abs
        .canonicalize()
        .map_err(|_| Error::Input(format!("rules file {} does not exist", given.display())))?;
    let root = repo_dir.canonicalize().map_err(io_err(repo_dir))?;
    let rel = abs.strip_prefix(&root).map_err(|_| {
        Error::Input(format!(
            "rules file {} is outside the repo {}",
            given.display(),
            repo_dir.display()
        ))
    })?;
    Ok(to_forward_slashes(rel))
}

/// Set up `.rules-evolve/` in a repo. Makes no model calls.
pub fn init(opts: &InitOptions) -> Result<InitReport> {
    if opts.model.trim().is_empty() {
        return Err(Error::Input("model name must not be empty".into()));
    }
    if !opts.dir.is_dir() {
        return Err(Error::Input(format!(
            "{} is not a directory",
            opts.dir.display()
        )));
    }
    let store = Store::for_repo(&opts.dir);
    if store.root().exists() {
        return Err(Error::AlreadyInitialized(store.root().to_path_buf()));
    }

    let (rules_rel, detected, others, notes) = match &opts.rules_file {
        Some(p) => (relativize(&opts.dir, p)?, false, Vec::new(), Vec::new()),
        None => {
            let d = detect_rules_file(&opts.dir)?;
            (d.chosen, true, d.others, d.notes)
        }
    };
    let rules_path = opts.dir.join(&rules_rel);
    if !rules_path.is_file() {
        return Err(Error::Input(format!(
            "rules file {rules_rel} is not a regular file"
        )));
    }
    let content = std::fs::read(&rules_path).map_err(io_err(&rules_path))?;

    store.create_dirs()?;
    let root_hash = store.put_blob(&content)?;

    let config = Config::with_defaults(&rules_rel, &opts.model);
    let config_path = store.config_path();
    std::fs::write(&config_path, config.to_toml_template()).map_err(io_err(&config_path))?;
    let tasks_path = store.tasks_path();
    std::fs::write(&tasks_path, TASKS_TEMPLATE).map_err(io_err(&tasks_path))?;

    let log = Log::new(store.root());
    log.append(&Payload::Init(InitPayload {
        rules_file: rules_rel.clone(),
        model: opts.model.clone(),
        root_hash: root_hash.clone(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
    }))?;

    let rel = |name: &str| format!("{DIR_NAME}/{name}");
    Ok(InitReport {
        repo_root: opts.dir.display().to_string(),
        rules_file: rules_rel,
        detected,
        other_candidates: others,
        notes,
        model: opts.model.clone(),
        root_hash: root_hash.clone(),
        defaults_confirmed: true,
        files_created: vec![
            rel("config.toml"),
            rel("tasks.toml"),
            rel("log.jsonl"),
            rel("HEAD"),
            rel(&format!("blobs/{root_hash}")),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::Repo;
    use tempfile::tempdir;

    fn opts(dir: &Path, rules: Option<&str>) -> InitOptions {
        InitOptions {
            dir: dir.to_path_buf(),
            rules_file: rules.map(PathBuf::from),
            model: "test-model".into(),
        }
    }

    #[test]
    fn init_detects_agents_md_and_writes_everything() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "be nice\n").unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "other\n").unwrap();
        let report = init(&opts(dir.path(), None)).unwrap();
        assert_eq!(report.rules_file, "AGENTS.md");
        assert!(report.detected);
        assert_eq!(report.other_candidates, vec!["CLAUDE.md".to_string()]);
        assert!(report.defaults_confirmed);

        let store = Store::for_repo(dir.path());
        assert!(store.config_path().is_file());
        assert!(store.tasks_path().is_file());
        assert_eq!(store.get_blob(&report.root_hash).unwrap(), b"be nice\n");

        let cfg = Config::load(&store.config_path()).unwrap();
        assert_eq!(cfg.evaluation.repeats, 5);
        assert!(cfg.meta.defaults_confirmed);

        let repo = Repo::open(dir.path()).unwrap();
        let status = repo.status().unwrap();
        assert!(status.valid);
        let tree = status.tree.unwrap();
        assert_eq!(tree.nodes.len(), 1);
        assert_eq!(tree.root, report.root_hash);
        assert_eq!(tree.model, "test-model");
    }

    #[test]
    fn init_falls_back_to_claude_md_then_cursor_rules() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "x").unwrap();
        assert_eq!(detect_rules_file(dir.path()).unwrap().chosen, "CLAUDE.md");

        let dir2 = tempdir().unwrap();
        std::fs::create_dir_all(dir2.path().join(".cursor/rules")).unwrap();
        std::fs::write(dir2.path().join(".cursor/rules/main.mdc"), "x").unwrap();
        assert_eq!(
            detect_rules_file(dir2.path()).unwrap().chosen,
            ".cursor/rules/main.mdc"
        );
    }

    #[test]
    fn cursor_rules_folder_with_many_files_needs_explicit_choice() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".cursor/rules")).unwrap();
        std::fs::write(dir.path().join(".cursor/rules/a.mdc"), "x").unwrap();
        std::fs::write(dir.path().join(".cursor/rules/b.mdc"), "y").unwrap();
        let err = detect_rules_file(dir.path()).unwrap_err();
        assert!(err.to_string().contains("--rules-file"));
        let report = init(&opts(dir.path(), Some(".cursor/rules/b.mdc"))).unwrap();
        assert_eq!(report.rules_file, ".cursor/rules/b.mdc");
        assert!(!report.detected);
    }

    #[test]
    fn init_fails_when_nothing_found_or_already_initialized() {
        let dir = tempdir().unwrap();
        assert!(init(&opts(dir.path(), None)).is_err());
        std::fs::write(dir.path().join("AGENTS.md"), "x").unwrap();
        init(&opts(dir.path(), None)).unwrap();
        let err = init(&opts(dir.path(), None)).unwrap_err();
        assert!(matches!(err, Error::AlreadyInitialized(_)));
    }

    #[test]
    fn init_rejects_missing_or_outside_rules_file() {
        let dir = tempdir().unwrap();
        let other = tempdir().unwrap();
        std::fs::write(other.path().join("AGENTS.md"), "x").unwrap();
        assert!(init(&opts(dir.path(), Some("nope.md"))).is_err());
        let outside = other.path().join("AGENTS.md");
        let err = init(&InitOptions {
            dir: dir.path().to_path_buf(),
            rules_file: Some(outside),
            model: "m".into(),
        })
        .unwrap_err();
        assert!(err.to_string().contains("outside the repo"));
    }

    #[test]
    fn init_rejects_empty_model() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "x").unwrap();
        let mut o = opts(dir.path(), None);
        o.model = "  ".into();
        assert!(init(&o).is_err());
    }
}
