//! Project config (`config.toml`) and the task file template (`tasks.toml`).

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub rules_file: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationConfig {
    /// Runs per version. The median is taken over these.
    pub repeats: u32,
    /// A child must beat its parent by this many percentage points of median pass rate.
    pub accept_threshold_pp: f64,
    /// And win at least this many of the paired runs.
    pub min_paired_wins: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetConfig {
    pub max_tokens_per_run: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetaConfig {
    /// `false` until the user has confirmed the evaluation and budget values.
    pub defaults_confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub project: ProjectConfig,
    pub evaluation: EvaluationConfig,
    pub budget: BudgetConfig,
    pub meta: MetaConfig,
}

fn toml_string(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

impl Config {
    pub fn with_defaults(rules_file: &str, model: &str) -> Config {
        Config {
            project: ProjectConfig {
                rules_file: rules_file.to_string(),
                model: model.to_string(),
            },
            evaluation: EvaluationConfig {
                repeats: 5,
                accept_threshold_pp: 10.0,
                min_paired_wins: 4,
            },
            budget: BudgetConfig {
                max_tokens_per_run: 200_000,
            },
            meta: MetaConfig {
                defaults_confirmed: false,
            },
        }
    }

    /// The text written to `config.toml`, with comments.
    pub fn to_toml_template(&self) -> String {
        format!(
            "# rules-evolve config\n\
             #\n\
             # The [evaluation] and [budget] values below are DEFAULTS that have\n\
             # NOT been confirmed by the user yet. Change them if you like, then set\n\
             # defaults_confirmed = true in [meta].\n\
             \n\
             [project]\n\
             # Rules file to evolve, relative to the repo root.\n\
             rules_file = {rules_file}\n\
             # Model name. Only recorded. Phase 1 makes no model calls.\n\
             model = {model}\n\
             \n\
             [evaluation]\n\
             # Runs per version. The median pass rate is used.\n\
             repeats = {repeats}\n\
             # A child must beat its parent by this many percentage points on the median pass rate...\n\
             accept_threshold_pp = {threshold:?}\n\
             # ...and win at least this many of the paired runs. Otherwise the result is noise.\n\
             min_paired_wins = {wins}\n\
             \n\
             [budget]\n\
             # Token cap for one run.\n\
             max_tokens_per_run = {tokens}\n\
             \n\
             [meta]\n\
             defaults_confirmed = {confirmed}\n",
            rules_file = toml_string(&self.project.rules_file),
            model = toml_string(&self.project.model),
            repeats = self.evaluation.repeats,
            threshold = self.evaluation.accept_threshold_pp,
            wins = self.evaluation.min_paired_wins,
            tokens = self.budget.max_tokens_per_run,
            confirmed = self.meta.defaults_confirmed,
        )
    }

    pub fn load(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        toml::from_str(&text).map_err(|e| Error::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }
}

/// One benchmark task: a prompt for the agent and a shell command that
/// decides pass or fail (exit code 0 means pass).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub prompt: String,
    pub check: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TasksFile {
    #[serde(default)]
    pub task: Vec<Task>,
}

impl TasksFile {
    pub fn load(path: &Path) -> Result<TasksFile> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        toml::from_str(&text).map_err(|e| Error::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }
}

/// Template written to `tasks.toml`. It holds one placeholder task.
pub const TASKS_TEMPLATE: &str = r#"# rules-evolve tasks
#
# Each [[task]] is one small job taken from your repo. The agent gets the
# prompt. Then the check command runs in the repo. Exit code 0 means pass.
#
# Good tasks come from your own history: a test that used to fail, or an
# issue with a known fix. Keep the check command out of the agent's reach.
#
# Phase 1 only creates this file. Nothing runs these tasks yet.
#
# The task below is a PLACEHOLDER. Replace it with real tasks.

[[task]]
id = "example-fix-failing-test"
prompt = "The test suite has one failing test. Fix the code so all tests pass."
check = "cargo test --quiet"
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_round_trips_to_defaults() {
        let cfg = Config::with_defaults("AGENTS.md", "some-model");
        let text = cfg.to_toml_template();
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(parsed, cfg);
        assert_eq!(parsed.evaluation.repeats, 5);
        assert_eq!(parsed.evaluation.accept_threshold_pp, 10.0);
        assert_eq!(parsed.evaluation.min_paired_wins, 4);
        assert_eq!(parsed.budget.max_tokens_per_run, 200_000);
        assert!(!parsed.meta.defaults_confirmed);
    }

    #[test]
    fn template_handles_quotes_in_model_name() {
        let cfg = Config::with_defaults("AGENTS.md", "weird \"name\"");
        let parsed: Config = toml::from_str(&cfg.to_toml_template()).unwrap();
        assert_eq!(parsed.project.model, "weird \"name\"");
    }

    #[test]
    fn tasks_template_parses() {
        let parsed: TasksFile = toml::from_str(TASKS_TEMPLATE).unwrap();
        assert_eq!(parsed.task.len(), 1);
        assert_eq!(parsed.task[0].id, "example-fix-failing-test");
    }
}
