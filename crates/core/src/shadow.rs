//! Warnings about layouts where model-written files could shadow or change
//! hidden tests.
//!
//! Hidden files are mounted read only during the check (see the README), but a
//! folder that is not itself hidden stays writable, so code the model wrote can
//! add a file the test runner picks up first (a `conftest.py`, an
//! `__init__.py`, a module with the same name). The same is true of runner
//! config the model can see and edit. [`analyze`] finds those layouts when the
//! sandbox is scanned and says how to fix each one in plain words.
//!
//! Three cases are covered:
//!
//! 1. a hidden file in a folder that is not hidden and holds visible files
//!    ([`ShadowKind::HiddenFileBesideVisibleFiles`]);
//! 2. a hidden file in a folder with no visible files, which the model can
//!    still add files to ([`ShadowKind::WritableFolderAroundHiddenFile`]);
//! 3. well-known runner config that is visible and editable, when the check
//!    uses the tool that reads it ([`ShadowKind::EditableRunnerConfig`]).
//!
//! The list for case 3 is [`CONFIG_RULES`]: add a row to cover another tool.
//! Hidden files at the repo root are not flagged under cases 1 and 2, because
//! the root cannot be hidden as a folder. Case 3 covers the config that matters
//! there.
//!
//! Warnings never stop a run unless `[sandbox] shadow_warnings = "error"`.

use crate::error::{Error, Result};
use crate::repo::Repo;
use crate::sandbox::{path_matches, Sandbox};
use crate::tasks::TasksFile;
use crate::tree::Tree;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What to do with shadow warnings. Set with `[sandbox] shadow_warnings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShadowMode {
    /// Show them and carry on. The default.
    #[default]
    Warn,
    /// Refuse to run while there is any.
    Error,
    /// Do not look.
    Off,
}

impl ShadowMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ShadowMode::Warn => "warn",
            ShadowMode::Error => "error",
            ShadowMode::Off => "off",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowKind {
    HiddenFileBesideVisibleFiles,
    WritableFolderAroundHiddenFile,
    EditableRunnerConfig,
}

/// One risky layout, with the fix in plain words.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShadowWarning {
    pub kind: ShadowKind,
    /// The paths involved, relative to the repo root.
    pub paths: Vec<String>,
    pub message: String,
    pub fix: String,
}

impl ShadowWarning {
    /// Two lines for people.
    pub fn text(&self) -> String {
        format!("warning: {}\n  fix: {}", self.message, self.fix)
    }
}

/// A tool whose config the model could edit, and where that config lives.
pub struct ConfigRule {
    /// Shown in messages.
    pub name: &'static str,
    /// Words that mean the check uses this tool: a command name in a task's
    /// `check`, or in a hidden shell script. A version suffix such as
    /// `python3.12` counts.
    pub tools: &'static [&'static str],
    /// Config files and where they may sit, in the same glob form as `hidden`.
    pub files: &'static [&'static str],
    /// What a change to these files can do.
    pub why: &'static str,
}

/// The data behind case 3. To cover another tool, add a row.
pub const CONFIG_RULES: &[ConfigRule] = &[
    ConfigRule {
        name: "Python",
        tools: &[
            "pytest", "py.test", "python", "python3", "tox", "nox", "uv", "poetry",
        ],
        files: &[
            "**/conftest.py",
            "**/pytest.ini",
            "**/pyproject.toml",
            "**/setup.cfg",
            "**/tox.ini",
            "**/noxfile.py",
            "**/sitecustomize.py",
        ],
        why: "pytest and tox load these on their own, so a change can skip, rewrite or fake tests",
    },
    ConfigRule {
        name: "JavaScript",
        tools: &[
            "npm", "npx", "node", "yarn", "pnpm", "bun", "jest", "vitest", "mocha",
        ],
        files: &[
            "**/package.json",
            "**/jest.config.*",
            "**/vitest.config.*",
            "**/vite.config.*",
            "**/.mocharc*",
            "**/babel.config.*",
            "**/.babelrc*",
            "**/tsconfig.json",
            "**/.npmrc",
        ],
        why: "package.json scripts and these configs decide which tests run and with what setup",
    },
    ConfigRule {
        name: "Rust",
        tools: &["cargo", "rustc"],
        files: &[
            "**/Cargo.toml",
            "**/build.rs",
            "**/.cargo/config.toml",
            "**/.cargo/config",
            "**/rust-toolchain*",
        ],
        why: "build.rs and Cargo.toml run code and choose what gets built and tested",
    },
    ConfigRule {
        name: "Make",
        tools: &["make", "gmake"],
        files: &["**/Makefile", "**/makefile", "**/GNUmakefile", "**/*.mk"],
        why: "a Makefile is a script, so a change can make a target pass without running anything",
    },
    ConfigRule {
        name: "Go",
        tools: &["go"],
        files: &["**/go.mod", "**/go.work"],
        why: "go.mod and go.work choose the modules and replacements the tests build against",
    },
    ConfigRule {
        name: "Java",
        tools: &["mvn", "gradle", "gradlew", "ant"],
        files: &[
            "**/pom.xml",
            "**/build.gradle*",
            "**/settings.gradle*",
            "**/build.xml",
        ],
        why: "build files decide which tests run and can run code of their own",
    },
    ConfigRule {
        name: "Ruby",
        tools: &["rspec", "rake", "bundle", "ruby"],
        files: &["**/Rakefile", "**/.rspec", "**/Gemfile"],
        why: "Rakefile and .rspec choose which specs run and can run code of their own",
    },
];

/// Most paths named in a message. The `paths` array holds more.
const SHOWN: usize = 5;
const PATHS_CAP: usize = 20;
/// Hidden shell scripts read for tool names: size and count limits.
const SCRIPT_BYTES: u64 = 64 * 1024;
const SCRIPTS_MAX: usize = 200;

fn parent_dir(rel: &str) -> &str {
    rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

fn list(paths: &[String]) -> String {
    let mut shown: Vec<&str> = paths.iter().take(SHOWN).map(|s| s.as_str()).collect();
    let extra = paths.len().saturating_sub(SHOWN);
    let tail = if extra > 0 {
        format!(" and {extra} more")
    } else {
        String::new()
    };
    shown.dedup();
    format!("{}{tail}", shown.join(", "))
}

fn capped(paths: Vec<String>) -> Vec<String> {
    paths.into_iter().take(PATHS_CAP).collect()
}

/// Words in a command or script that could name a tool.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

fn names_tool(word: &str, tool: &str) -> bool {
    // Exact, or with a version on the end: python3, python3.12.
    word.strip_prefix(tool)
        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

fn looks_like_script(rel: &str, first_line: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.ends_with(".sh")
        || name.ends_with(".bash")
        || name.ends_with(".zsh")
        || name == "Makefile"
        || name.ends_with(".mk")
        || first_line.starts_with("#!")
}

/// Look for risky layouts in a scanned sandbox. `tasks` supplies the check
/// commands.
pub fn analyze(sandbox: &Sandbox, tasks: &TasksFile) -> Vec<ShadowWarning> {
    let mut out = Vec::new();

    // Cases 1 and 2: hidden files in folders that are not hidden.
    let mut by_folder: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for f in sandbox.hidden_files() {
        if sandbox.is_under_hidden_dir(f) {
            continue;
        }
        let dir = parent_dir(f);
        if !dir.is_empty() {
            by_folder.entry(dir).or_default().push(f.clone());
        }
    }
    for (dir, hidden) in &by_folder {
        let visible: Vec<String> = sandbox
            .visible_files()
            .iter()
            .filter(|v| parent_dir(v) == *dir)
            .cloned()
            .collect();
        let mut paths = vec![(*dir).to_string()];
        paths.extend(hidden.iter().cloned());
        if visible.is_empty() {
            out.push(ShadowWarning {
                kind: ShadowKind::WritableFolderAroundHiddenFile,
                paths: capped(paths),
                message: format!(
                    "{dir}/ holds the hidden file(s) {} and no visible files, but the model can still create files there. Code the model writes could add a conftest.py, an __init__.py, a pytest.ini or a module with the same name, and the test runner could pick it up.",
                    list(hidden)
                ),
                fix: format!(
                    "Add \"{dir}\" to hidden in tasks.toml so the whole folder is hidden. A hidden folder is mounted read only as a whole, so nothing can be added to it."
                ),
            });
        } else {
            out.push(ShadowWarning {
                kind: ShadowKind::HiddenFileBesideVisibleFiles,
                paths: capped(paths),
                message: format!(
                    "{dir}/ holds the hidden file(s) {} next to visible files ({}). Code the model writes could add a file beside the hidden one, such as a conftest.py, an __init__.py or a module with the same name, and the test runner could pick it up first.",
                    list(hidden),
                    list(&visible)
                ),
                fix: format!(
                    "Add \"{dir}\" to hidden in tasks.toml so the whole folder is hidden and read only. If the visible files there must stay visible, move the hidden files into a folder of their own and hide that folder."
                ),
            });
        }
    }

    // Case 3: runner config the model can see and edit, for tools the check uses.
    let mut seen_words: Vec<String> = Vec::new();
    for t in &tasks.task {
        seen_words.extend(words(&t.check));
    }
    for rel in sandbox.hidden_files().iter().take(SCRIPTS_MAX) {
        let path = sandbox.pristine_path(rel);
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if meta.len() > SCRIPT_BYTES {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if looks_like_script(rel, text.lines().next().unwrap_or("")) {
            seen_words.extend(words(&text));
        }
    }
    for rule in CONFIG_RULES {
        let used: Vec<&str> = rule
            .tools
            .iter()
            .copied()
            .filter(|tool| seen_words.iter().any(|w| names_tool(w, tool)))
            .collect();
        if used.is_empty() {
            continue;
        }
        let mut files: Vec<String> = sandbox
            .visible_files()
            .iter()
            .filter(|f| rule.files.iter().any(|p| path_matches(p, f)))
            .cloned()
            .collect();
        files.sort();
        if files.is_empty() {
            continue;
        }
        out.push(ShadowWarning {
            kind: ShadowKind::EditableRunnerConfig,
            message: format!(
                "The check uses {} and the model can see and edit its config: {}. {}.",
                used.join(", "),
                list(&files),
                rule.why
            ),
            fix: "Add these files to hidden in tasks.toml so the model cannot edit them (the check still gets the originals), or to exclude if the check does not need them. If the model has to see one, keep it visible and accept that code it writes can change how the tests run.".to_string(),
            paths: capped(files),
        });
    }
    out
}

/// Apply the mode: `Off` drops everything, `Error` turns any warning into an
/// error that names them all, `Warn` returns them for display.
pub fn apply_mode(mode: ShadowMode, warnings: Vec<ShadowWarning>) -> Result<Vec<ShadowWarning>> {
    match mode {
        ShadowMode::Off => Ok(Vec::new()),
        ShadowMode::Warn => Ok(warnings),
        ShadowMode::Error => {
            if warnings.is_empty() {
                return Ok(warnings);
            }
            let mut text = String::from(
                "shadow file risks were found and shadow_warnings = \"error\" in [sandbox] of config.toml, so nothing was run:",
            );
            for w in &warnings {
                text.push_str(&format!("\n  - {}\n    fix: {}", w.message, w.fix));
            }
            Err(Error::Input(text))
        }
    }
}

/// Scan the repo and analyze it, ignoring the mode (for `isolation` and `init`,
/// which only report). `Off` still returns nothing.
pub fn scan_repo(repo: &Repo) -> Result<(ShadowMode, Vec<ShadowWarning>)> {
    let mode = repo.sandbox_settings()?.shadow_warnings;
    if mode == ShadowMode::Off {
        return Ok((mode, Vec::new()));
    }
    let tasks = repo.tasks()?;
    let rules_file = Tree::from_events(&repo.events()?)?.rules_file;
    let sandbox = Sandbox::scan(repo.dir(), &rules_file, &tasks.sandbox)?;
    Ok((mode, analyze(&sandbox, &tasks)))
}

/// Scan the repo and apply the configured mode. Used before a run starts.
pub fn check_repo(repo: &Repo) -> Result<Vec<ShadowWarning>> {
    let (mode, warnings) = scan_repo(repo)?;
    apply_mode(mode, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{SandboxConfig, Task};
    use std::path::Path;
    use tempfile::tempdir;

    fn write(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    fn task(check: &str) -> Task {
        Task {
            id: "t".into(),
            prompt: "p".into(),
            check: check.into(),
            timeout_secs: None,
            split: Default::default(),
            context: Vec::new(),
        }
    }

    fn tasks(hidden: &[&str], exclude: Option<&[&str]>, checks: &[&str]) -> TasksFile {
        let mut sandbox = SandboxConfig {
            hidden: hidden.iter().map(|s| s.to_string()).collect(),
            ..SandboxConfig::default()
        };
        if let Some(e) = exclude {
            sandbox.exclude = e.iter().map(|s| s.to_string()).collect();
        }
        TasksFile {
            sandbox,
            task: checks.iter().map(|c| task(c)).collect(),
        }
    }

    fn run(root: &Path, t: &TasksFile) -> Vec<ShadowWarning> {
        let sb = Sandbox::scan(root, "AGENTS.md", &t.sandbox).unwrap();
        analyze(&sb, t)
    }

    fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempdir().unwrap();
        write(d.path(), "AGENTS.md", "rules");
        for (p, c) in files {
            write(d.path(), p, c);
        }
        d
    }

    // ---- case 1 ----

    #[test]
    fn a_hidden_file_beside_visible_files_is_warned_about() {
        let d = repo(&[
            ("docs/spec.txt", "s"),
            ("docs/readme.txt", "r"),
            ("docs/more.txt", "m"),
        ]);
        let w = run(d.path(), &tasks(&["docs/spec.txt"], None, &["sh check.sh"]));
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(w[0].kind, ShadowKind::HiddenFileBesideVisibleFiles);
        assert_eq!(w[0].paths, vec!["docs", "docs/spec.txt"]);
        assert!(w[0].message.contains("docs/spec.txt"));
        assert!(
            w[0].message.contains("docs/more.txt, docs/readme.txt"),
            "{}",
            w[0].message
        );
        assert!(w[0].message.contains("conftest.py"));
        assert!(
            w[0].fix.contains("Add \"docs\" to hidden in tasks.toml"),
            "{}",
            w[0].fix
        );
    }

    #[test]
    fn several_hidden_files_in_one_folder_make_one_warning() {
        let d = repo(&[("t/a.py", "1"), ("t/b.py", "2"), ("t/c.py", "3")]);
        let w = run(d.path(), &tasks(&["t/a.py", "t/b.py"], None, &[]));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].paths, vec!["t", "t/a.py", "t/b.py"]);
    }

    // ---- case 2 ----

    #[test]
    fn a_folder_with_only_hidden_files_is_still_writable() {
        let d = repo(&[("tests/test_a.py", "a")]);
        let w = run(d.path(), &tasks(&["tests/test_a.py"], None, &[]));
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(w[0].kind, ShadowKind::WritableFolderAroundHiddenFile);
        assert!(w[0].message.contains("no visible files"));
        assert!(w[0].message.contains("__init__.py"));
        assert!(w[0].fix.contains("Add \"tests\" to hidden"));
    }

    #[test]
    fn a_glob_that_hides_every_file_of_a_folder_still_leaves_it_writable() {
        let d = repo(&[("tests/test_a.py", "a"), ("tests/test_b.py", "b")]);
        let w = run(d.path(), &tasks(&["tests/test_*.py"], None, &[]));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].kind, ShadowKind::WritableFolderAroundHiddenFile);
    }

    // ---- no false warnings ----

    #[test]
    fn hiding_the_whole_folder_gives_no_warning() {
        let d = repo(&[
            ("tests/test_a.py", "a"),
            ("tests/sub/test_b.py", "b"),
            ("src/lib.py", "x"),
            ("docs/readme.txt", "r"),
        ]);
        let w = run(d.path(), &tasks(&["tests"], None, &["pytest -q"]));
        assert!(w.is_empty(), "{w:?}");
        // The same through a glob that names the folder.
        let w = run(d.path(), &tasks(&["tests/**"], None, &[]));
        assert!(
            w.is_empty()
                || w.iter()
                    .all(|x| x.kind != ShadowKind::HiddenFileBesideVisibleFiles)
        );
    }

    #[test]
    fn hidden_files_at_the_repo_root_are_not_a_folder_risk() {
        let d = repo(&[
            ("check.sh", "exit 0"),
            ("README.md", "r"),
            ("src/a.py", "a"),
        ]);
        let w = run(d.path(), &tasks(&["check.sh"], None, &["sh check.sh"]));
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn nothing_hidden_gives_no_folder_warning() {
        let d = repo(&[("src/a.py", "a")]);
        assert!(run(d.path(), &tasks(&[], None, &["sh x.sh"])).is_empty());
    }

    // ---- case 3 ----

    #[test]
    fn visible_runner_config_for_a_tool_the_check_uses_is_warned_about() {
        let d = repo(&[
            ("conftest.py", "c"),
            ("pyproject.toml", "p"),
            ("pkg/pytest.ini", "i"),
            ("src/app.py", "a"),
        ]);
        let w = run(d.path(), &tasks(&[], None, &["python3.12 -m pytest -q"]));
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(w[0].kind, ShadowKind::EditableRunnerConfig);
        assert_eq!(
            w[0].paths,
            vec!["conftest.py", "pkg/pytest.ini", "pyproject.toml"]
        );
        assert!(w[0].message.contains("python") && w[0].message.contains("pytest"));
        assert!(w[0].fix.contains("hidden") && w[0].fix.contains("exclude"));
    }

    #[test]
    fn config_is_found_through_a_hidden_script_that_runs_the_tool() {
        let d = repo(&[
            ("run_tests.sh", "#!/bin/sh\ncd web && npx jest --ci\n"),
            ("web/package.json", "{}"),
            ("web/jest.config.js", ""),
        ]);
        let w = run(
            d.path(),
            &tasks(&["run_tests.sh"], None, &["sh run_tests.sh"]),
        );
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(w[0].paths, vec!["web/jest.config.js", "web/package.json"]);
        // A script that does not name a tool says nothing.
        let d = repo(&[
            ("run_tests.sh", "#!/bin/sh\nexit 0\n"),
            ("package.json", "{}"),
        ]);
        assert!(run(
            d.path(),
            &tasks(&["run_tests.sh"], None, &["sh run_tests.sh"])
        )
        .is_empty());
    }

    #[test]
    fn config_that_is_hidden_or_excluded_or_not_used_gives_no_warning() {
        let files = [("conftest.py", "c"), ("Makefile", "m"), ("Cargo.toml", "t")];
        let d = repo(&files);
        // Hidden: the model cannot edit it.
        let w = run(d.path(), &tasks(&["conftest.py"], None, &["pytest"]));
        assert!(
            w.iter().all(|x| x.kind != ShadowKind::EditableRunnerConfig),
            "{w:?}"
        );
        // Excluded: it is not in the worktree at all.
        let w = run(d.path(), &tasks(&[], Some(&["conftest.py"]), &["pytest"]));
        assert!(w.is_empty(), "{w:?}");
        // The check does not use the tools.
        let w = run(d.path(), &tasks(&[], None, &["sh build.sh", "echo hello"]));
        assert!(w.is_empty(), "{w:?}");
        // A word that only starts like a tool name is not the tool.
        let w = run(
            d.path(),
            &tasks(&[], None, &["cargotool --x", "maker", "pytester"]),
        );
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn each_tool_gets_its_own_warning_and_the_list_is_capped() {
        let mut files: Vec<(String, String)> = (0..8)
            .map(|i| (format!("p{i}/package.json"), "{}".to_string()))
            .collect();
        files.push(("Cargo.toml".into(), "t".into()));
        files.push(("Makefile".into(), "m".into()));
        let d = tempdir().unwrap();
        write(d.path(), "AGENTS.md", "r");
        for (p, c) in &files {
            write(d.path(), p, c);
        }
        let w = run(
            d.path(),
            &tasks(&[], None, &["make test && cargo test && npm test"]),
        );
        let kinds: Vec<&ShadowKind> = w.iter().map(|x| &x.kind).collect();
        assert_eq!(kinds, vec![&ShadowKind::EditableRunnerConfig; 3]);
        let js = w
            .iter()
            .find(|x| x.message.contains("JavaScript") || x.message.contains("npm"))
            .unwrap();
        assert_eq!(js.paths.len(), 8);
        assert!(js.message.contains("and 3 more"), "{}", js.message);
    }

    #[test]
    fn the_config_table_is_sane() {
        for r in CONFIG_RULES {
            assert!(
                !r.name.is_empty()
                    && !r.tools.is_empty()
                    && !r.files.is_empty()
                    && !r.why.is_empty()
            );
            for pat in r.files {
                assert!(pat.starts_with("**/"), "{pat}");
                // Each pattern matches a file of that name at the root and nested.
                let name = pat.trim_start_matches("**/").replace('*', "x");
                assert!(path_matches(pat, &name), "{pat} vs {name}");
                assert!(path_matches(pat, &format!("a/b/{name}")), "{pat}");
            }
            assert!(!r.why.contains('\u{2014}') && !r.why.contains('\u{2013}'));
        }
    }

    // ---- mode, text and JSON ----

    fn sample() -> Vec<ShadowWarning> {
        let d = repo(&[("docs/spec.txt", "s"), ("docs/readme.txt", "r")]);
        run(d.path(), &tasks(&["docs/spec.txt"], None, &[]))
    }

    #[test]
    fn modes_warn_error_and_off() {
        let w = sample();
        assert_eq!(w.len(), 1);
        assert_eq!(apply_mode(ShadowMode::Warn, w.clone()).unwrap(), w);
        assert!(apply_mode(ShadowMode::Off, w.clone()).unwrap().is_empty());
        let err = apply_mode(ShadowMode::Error, w.clone())
            .unwrap_err()
            .to_string();
        assert!(err.contains("shadow_warnings = \"error\""), "{err}");
        assert!(err.contains("nothing was run"));
        assert!(err.contains(&w[0].message) && err.contains(&w[0].fix));
        // Error mode with nothing to report is fine.
        assert!(apply_mode(ShadowMode::Error, Vec::new())
            .unwrap()
            .is_empty());
        assert_eq!(ShadowMode::default(), ShadowMode::Warn);
    }

    #[test]
    fn warnings_serialize_with_a_stable_shape() {
        let w = sample();
        let v = serde_json::to_value(&w).unwrap();
        let first = &v[0];
        let mut keys: Vec<&String> = first.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, ["fix", "kind", "message", "paths"]);
        assert_eq!(first["kind"], "hidden_file_beside_visible_files");
        assert!(first["paths"].is_array());
        assert_eq!(
            serde_json::to_value(ShadowKind::WritableFolderAroundHiddenFile).unwrap(),
            "writable_folder_around_hidden_file"
        );
        assert_eq!(
            serde_json::to_value(ShadowKind::EditableRunnerConfig).unwrap(),
            "editable_runner_config"
        );
        let t = w[0].text();
        assert!(t.starts_with("warning: ") && t.contains("\n  fix: "));
    }

    #[test]
    fn mode_names_parse_from_config() {
        #[derive(Deserialize)]
        struct C {
            m: ShadowMode,
        }
        for (s, m) in [
            ("warn", ShadowMode::Warn),
            ("error", ShadowMode::Error),
            ("off", ShadowMode::Off),
        ] {
            let c: C = toml::from_str(&format!("m = \"{s}\"")).unwrap();
            assert_eq!(c.m, m);
            assert_eq!(m.as_str(), s);
        }
        assert!(toml::from_str::<C>("m = \"loud\"").is_err());
    }
}
