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
pub struct MutationConfig {
    /// Largest allowed change, in bytes, between a parent and a child rules file.
    pub max_change_bytes: usize,
}

impl Default for MutationConfig {
    fn default() -> Self {
        MutationConfig {
            max_change_bytes: 1000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetaConfig {
    /// `true` once the evaluation and budget values have been confirmed.
    pub defaults_confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub project: ProjectConfig,
    pub evaluation: EvaluationConfig,
    pub budget: BudgetConfig,
    #[serde(default)]
    pub mutation: MutationConfig,
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
            mutation: MutationConfig::default(),
            meta: MetaConfig {
                defaults_confirmed: true,
            },
        }
    }

    /// The text written to `config.toml`, with comments.
    pub fn to_toml_template(&self) -> String {
        format!(
            "# rules-evolve config\n\
             #\n\
             # The [evaluation] and [budget] values below were confirmed by the project owner.\n\
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
             [mutation]\n\
             # Largest allowed change between parent and child, in bytes.\n\
             max_change_bytes = {max_change}\n\
             \n\
             [meta]\n\
             defaults_confirmed = {confirmed}\n",
            rules_file = toml_string(&self.project.rules_file),
            model = toml_string(&self.project.model),
            repeats = self.evaluation.repeats,
            threshold = self.evaluation.accept_threshold_pp,
            wins = self.evaluation.min_paired_wins,
            tokens = self.budget.max_tokens_per_run,
            max_change = self.mutation.max_change_bytes,
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
        assert!(parsed.meta.defaults_confirmed);
    }

    #[test]
    fn template_handles_quotes_in_model_name() {
        let cfg = Config::with_defaults("AGENTS.md", "weird \"name\"");
        let parsed: Config = toml::from_str(&cfg.to_toml_template()).unwrap();
        assert_eq!(parsed.project.model, "weird \"name\"");
    }
}
