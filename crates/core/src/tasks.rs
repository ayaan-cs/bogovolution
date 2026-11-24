//! `tasks.toml`: the benchmark tasks and the sandbox settings.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

/// One benchmark task: a prompt for the model and a shell command that
/// decides pass or fail (exit code 0 means pass).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    /// Letters, digits, `_`, `-` and `.` only.
    pub id: String,
    /// What the model is asked to do. The model never sees `check`.
    pub prompt: String,
    /// Shell command run after the model's edits are applied. Exit 0 passes.
    pub check: String,
    /// Seconds before the check is killed. Falls back to `default_timeout_secs`.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// `train` (the default) or `held_out`. Held out tasks are run and scored
    /// with every version, but their prompts, results and failure notes never
    /// reach the mutation prompt, and a child is accepted only if it also
    /// gains on them (see "The verdict rule").
    #[serde(default, skip_serializing_if = "Split::is_train")]
    pub split: Split,
    /// Repo files (or folders, ending in `/`) the model is shown for this
    /// task, relative to the repo root. Empty (the default) shows every
    /// readable file, as before. A short list keeps the prompt small when the
    /// repo holds many unrelated files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<String>,
}

/// Which part of the task set a task belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Split {
    #[default]
    Train,
    HeldOut,
}

impl Split {
    pub fn is_train(&self) -> bool {
        *self == Split::Train
    }
}

/// How the sandbox treats files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxConfig {
    /// Benchmark test and check files, as paths or globs relative to the repo
    /// root. They are left out of what the model can see, and put back only
    /// when the check runs. A path covers everything under it. `*` matches
    /// within one folder level, `**` matches any number of levels.
    #[serde(default)]
    pub hidden: Vec<String>,
    /// More paths or globs that are never copied. `.git` and `.graft`
    /// are always left out.
    #[serde(default = "default_exclude")]
    pub exclude: Vec<String>,
    /// Environment variables handed to check commands. Everything else is
    /// cleared, so API keys in your shell do not reach them. `PATH` is always
    /// kept.
    #[serde(default)]
    pub env_passthrough: Vec<String>,
    /// Check timeout for tasks that do not set their own.
    #[serde(default = "default_timeout")]
    pub default_timeout_secs: u64,
}

fn default_exclude() -> Vec<String> {
    vec!["target".into(), "node_modules".into()]
}

fn default_timeout() -> u64 {
    60
}

impl Default for SandboxConfig {
    fn default() -> Self {
        SandboxConfig {
            hidden: Vec::new(),
            exclude: default_exclude(),
            env_passthrough: Vec::new(),
            default_timeout_secs: default_timeout(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasksFile {
    #[serde(default)]
    pub sandbox: SandboxConfig,
    #[serde(default)]
    pub task: Vec<Task>,
}

fn bad_pattern(p: &str) -> bool {
    p.is_empty() || p.starts_with('/') || p.contains('\0') || p.split('/').any(|seg| seg == "..")
}

impl TasksFile {
    pub fn load(path: &Path) -> Result<TasksFile> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        Self::parse(&text).map_err(|e| Error::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }

    /// Parse and validate.
    pub fn parse(text: &str) -> std::result::Result<TasksFile, String> {
        let file: TasksFile = toml::from_str(text).map_err(|e| e.to_string())?;
        file.validate()?;
        Ok(file)
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        let mut seen = HashSet::new();
        for t in &self.task {
            if t.id.is_empty()
                || !t
                    .id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
            {
                return Err(format!(
                    "task id {:?} must use only letters, digits, _, - and .",
                    t.id
                ));
            }
            if !seen.insert(t.id.clone()) {
                return Err(format!("duplicate task id {:?}", t.id));
            }
            if t.prompt.trim().is_empty() {
                return Err(format!("task {} has an empty prompt", t.id));
            }
            if t.check.trim().is_empty() {
                return Err(format!("task {} has an empty check command", t.id));
            }
            if t.timeout_secs == Some(0) {
                return Err(format!("task {} has timeout_secs = 0", t.id));
            }
            for p in &t.context {
                if bad_pattern(p) {
                    return Err(format!(
                        "task {}: context path {p:?} must be relative to the repo root and must not contain ..",
                        t.id
                    ));
                }
            }
        }
        if !self.task.is_empty() && self.task.iter().all(|t| t.split == Split::HeldOut) {
            return Err(
                "every task is held_out: at least one task must be train (the default)".into(),
            );
        }
        if self.sandbox.default_timeout_secs == 0 {
            return Err("default_timeout_secs must be above 0".into());
        }
        for p in self
            .sandbox
            .hidden
            .iter()
            .chain(self.sandbox.exclude.iter())
        {
            if bad_pattern(p) {
                return Err(format!(
                    "path {p:?} must be relative to the repo root and must not contain .."
                ));
            }
        }
        Ok(())
    }

    /// True when at least one task is held out.
    pub fn has_held_out(&self) -> bool {
        self.task.iter().any(|t| t.split == Split::HeldOut)
    }

    /// Timeout in seconds for a task.
    pub fn timeout_for(&self, task: &Task) -> u64 {
        task.timeout_secs
            .unwrap_or(self.sandbox.default_timeout_secs)
    }
}

/// Template written to `tasks.toml`. It holds one placeholder task.
pub const TASKS_TEMPLATE: &str = r#"# graft tasks
#
# Each [[task]] is one small job taken from your repo. The model gets the
# prompt. Then the check command runs in a copy of the repo. Exit code 0 means
# pass. The model never sees the check command.
#
# Good tasks come from your own history: a test that used to fail, or an
# issue with a known fix.
#
# The [sandbox] section lists the benchmark test and check files. They are
# left out of the copy the model works in, and put back only when the check
# runs. Keep the real checking logic in those files, not in the check command
# text. See the README for the threat model and its limits.

[sandbox]
# Paths or globs relative to the repo root. A path covers everything under it.
hidden = ["tests"]
# Also never copied (.git and .graft are always left out).
exclude = ["target", "node_modules"]
# Environment variables check commands may see. Everything else is cleared.
env_passthrough = []
# Seconds before a check is killed.
default_timeout_secs = 60

# The task below is a PLACEHOLDER. Replace it with real tasks.
[[task]]
id = "example-fix-failing-test"
prompt = "The test suite has one failing test. Fix the code so all tests pass."
check = "cargo test --quiet"
# timeout_secs = 300
# Mark some tasks split = "held_out" to keep them out of every mutation
# prompt. A change is then accepted only if it also gains on them.
# split = "train"
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_parses_and_validates() {
        let parsed = TasksFile::parse(TASKS_TEMPLATE).unwrap();
        assert_eq!(parsed.task.len(), 1);
        assert_eq!(parsed.task[0].id, "example-fix-failing-test");
        assert_eq!(parsed.sandbox.hidden, vec!["tests".to_string()]);
        assert_eq!(parsed.timeout_for(&parsed.task[0]), 60);
    }

    #[test]
    fn split_defaults_to_train_and_parses_held_out() {
        let f = TasksFile::parse(
            "[[task]]\nid=\"a\"\nprompt=\"p\"\ncheck=\"true\"\n\
             [[task]]\nid=\"b\"\nprompt=\"p\"\ncheck=\"true\"\nsplit=\"held_out\"\n\
             [[task]]\nid=\"c\"\nprompt=\"p\"\ncheck=\"true\"\nsplit=\"train\"\n",
        )
        .unwrap();
        let splits: Vec<Split> = f.task.iter().map(|t| t.split).collect();
        assert_eq!(splits, vec![Split::Train, Split::HeldOut, Split::Train]);
        assert!(f.has_held_out());
        assert!(!TasksFile::parse(TASKS_TEMPLATE).unwrap().has_held_out());
        // a typo is an error, and so is a set with no train task
        assert!(TasksFile::parse(
            "[[task]]\nid=\"a\"\nprompt=\"p\"\ncheck=\"true\"\nsplit=\"heldout\"\n"
        )
        .is_err());
        let err = TasksFile::parse(
            "[[task]]\nid=\"a\"\nprompt=\"p\"\ncheck=\"true\"\nsplit=\"held_out\"\n",
        )
        .unwrap_err();
        assert!(err.contains("at least one task must be train"), "{err}");
    }

    #[test]
    fn task_timeout_overrides_default() {
        let f =
            TasksFile::parse("[[task]]\nid=\"a\"\nprompt=\"p\"\ncheck=\"true\"\ntimeout_secs=5\n")
                .unwrap();
        assert_eq!(f.timeout_for(&f.task[0]), 5);
        assert_eq!(f.sandbox.default_timeout_secs, 60);
        assert_eq!(f.sandbox.exclude, vec!["target", "node_modules"]);
    }

    #[test]
    fn rejects_bad_input() {
        let t = |extra: &str| TasksFile::parse(extra).is_err();
        assert!(t("[[task]]\nid=\"a b\"\nprompt=\"p\"\ncheck=\"c\"\n"));
        assert!(t("[[task]]\nid=\"a\"\nprompt=\"\"\ncheck=\"c\"\n"));
        assert!(t("[[task]]\nid=\"a\"\nprompt=\"p\"\ncheck=\" \"\n"));
        assert!(t(
            "[[task]]\nid=\"a\"\nprompt=\"p\"\ncheck=\"c\"\ntimeout_secs=0\n"
        ));
        assert!(t(
            "[[task]]\nid=\"a\"\nprompt=\"p\"\ncheck=\"c\"\n[[task]]\nid=\"a\"\nprompt=\"p\"\ncheck=\"c\"\n"
        ));
        assert!(t(
            "[[task]]\nid=\"a\"\nprompt=\"p\"\ncheck=\"c\"\nbogus=1\n"
        ));
        assert!(t("[sandbox]\nhidden=[\"../x\"]\n"));
        assert!(t("[sandbox]\nhidden=[\"/etc\"]\n"));
        assert!(t("[sandbox]\ndefault_timeout_secs=0\n"));
        assert!(t(
            "[[task]]\nid=\"a\"\nprompt=\"p\"\ncheck=\"c\"\ncontext=[\"../x.py\"]\n"
        ));
        assert!(t(
            "[[task]]\nid=\"a\"\nprompt=\"p\"\ncheck=\"c\"\ncontext=[\"/etc/passwd\"]\n"
        ));
        assert!(t(
            "[[task]]\nid=\"a\"\nprompt=\"p\"\ncheck=\"c\"\ncontext=[\"\"]\n"
        ));
    }

    #[test]
    fn context_defaults_to_empty_and_parses() {
        let f = TasksFile::parse(
            "[[task]]\nid=\"a\"\nprompt=\"p\"\ncheck=\"c\"\n\
             [[task]]\nid=\"b\"\nprompt=\"p\"\ncheck=\"c\"\ncontext=[\"solve/b.py\", \"lib/\"]\n",
        )
        .unwrap();
        assert!(f.task[0].context.is_empty());
        assert_eq!(f.task[1].context, vec!["solve/b.py", "lib/"]);
    }
}
