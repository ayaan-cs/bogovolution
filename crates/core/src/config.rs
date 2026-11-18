//! Project config (`config.toml`) and the task file template (`tasks.toml`).

use crate::error::{Error, Result};
use crate::isolation::SandboxSettings;
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
    /// Held out gate (used only when tasks.toml has held out tasks): the
    /// child must also gain this many points of held out median pass rate...
    #[serde(default = "default_held_out_threshold")]
    pub held_out_accept_threshold_pp: f64,
    /// ...and win at least this many paired runs on the held out tasks.
    #[serde(default = "default_held_out_wins")]
    pub held_out_min_paired_wins: u32,
    /// With at least this many held out tasks (0 turns it off), the held out
    /// gain needed is `held_out_large_set_threshold_pp` instead. With many
    /// held out tasks a run is less noisy, and a general rule rarely moves as
    /// many held out tasks as train tasks.
    #[serde(default = "default_held_out_large_set_tasks")]
    pub held_out_large_set_tasks: u32,
    #[serde(default = "default_held_out_large_set_threshold")]
    pub held_out_large_set_threshold_pp: f64,
    /// Model calls in one run that may fail (server gone, HTTP error) before
    /// the run is thrown away instead of recorded (v2.7.2). With the default
    /// 0, one failed call stops the run: nothing is recorded for it, and the
    /// same command started again resumes. A higher value records the run
    /// and counts the failed tasks as failures, as before v2.7.2.
    #[serde(default)]
    pub max_model_errors_per_run: u32,
}

impl EvaluationConfig {
    /// The held out gain needed for a task set with `held_out_tasks` held out
    /// tasks.
    pub fn held_out_threshold_for(&self, held_out_tasks: u32) -> f64 {
        if self.held_out_large_set_tasks > 0 && held_out_tasks >= self.held_out_large_set_tasks {
            self.held_out_large_set_threshold_pp
        } else {
            self.held_out_accept_threshold_pp
        }
    }
}

fn default_held_out_threshold() -> f64 {
    10.0
}

fn default_held_out_large_set_tasks() -> u32 {
    20
}

fn default_held_out_large_set_threshold() -> f64 {
    5.0
}

fn default_held_out_wins() -> u32 {
    3
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetConfig {
    pub max_tokens_per_run: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MutationConfig {
    /// Largest allowed change, in bytes, between a parent and a child rules file.
    pub max_change_bytes: usize,
    /// How many more times `evolve` asks the model, with a new seed, when a
    /// generation's mutation is refused (a repeat of a version already in the
    /// tree, an empty or oversized change, an unreadable answer) (v2.7.2).
    /// Every attempt is logged. 0 keeps the old behaviour: the generation
    /// ends as refused. Refusals for the token cap are never retried.
    #[serde(default)]
    pub retries_on_refusal: u32,
    /// Tell the mutation model that the agent answers each task in one reply
    /// and runs nothing, and which folders it cannot read (tasks.toml
    /// `[sandbox] hidden`), so it does not propose rules the agent cannot
    /// follow ("run the checks", "read the tests in checks/"). False keeps
    /// the old prompt, so replays recorded before this setting still match.
    #[serde(default)]
    pub one_shot_agent: bool,
}

impl Default for MutationConfig {
    fn default() -> Self {
        MutationConfig {
            max_change_bytes: 1000,
            retries_on_refusal: 0,
            one_shot_agent: false,
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
    /// Isolation of the check command. Old configs have no `[sandbox]` table.
    #[serde(default)]
    pub sandbox: SandboxSettings,
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
                held_out_accept_threshold_pp: default_held_out_threshold(),
                held_out_min_paired_wins: default_held_out_wins(),
                held_out_large_set_tasks: default_held_out_large_set_tasks(),
                held_out_large_set_threshold_pp: default_held_out_large_set_threshold(),
                max_model_errors_per_run: 0,
            },
            budget: BudgetConfig {
                max_tokens_per_run: 200_000,
            },
            mutation: MutationConfig::default(),
            sandbox: SandboxSettings::default(),
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
             # Only when tasks.toml marks tasks with split = \"held_out\": a child the rule above\n\
             # accepts must also gain this many points on the held out median...\n\
             held_out_accept_threshold_pp = {ho_threshold:?}\n\
             # ...and win this many held out pairs. Otherwise the verdict is overfit.\n\
             held_out_min_paired_wins = {ho_wins}\n\
             # With at least this many held out tasks (0 turns it off) the held out gain\n\
             # needed is held_out_large_set_threshold_pp instead of held_out_accept_threshold_pp.\n\
             held_out_large_set_tasks = {ho_large}\n\
             held_out_large_set_threshold_pp = {ho_large_threshold:?}\n\
             # Failed model calls (server gone, HTTP error) allowed in one run. Past this the\n\
             # run is not recorded and the command stops; run it again to resume.\n\
             max_model_errors_per_run = {model_errors}\n\
             \n\
             [budget]\n\
             # Token cap for one run.\n\
             max_tokens_per_run = {tokens}\n\
             \n\
             [mutation]\n\
             # Largest allowed change between parent and child, in bytes.\n\
             max_change_bytes = {max_change}\n\
             # evolve asks again, with a new seed, this many times when a mutation is refused\n\
             # (a repeat, an empty or oversized change, an unreadable answer).\n\
             retries_on_refusal = {retries}\n\
             # true: tell the mutation model the agent answers in one reply, runs nothing\n\
             # and cannot read hidden paths (right for the local and API providers).\n\
             # false keeps the older prompt, which older replay files need.\n\
             one_shot_agent = {one_shot}\n\
             \n\
             [sandbox]\n\
             # How check commands are isolated: auto, bwrap, docker, podman or none.\n\
             # auto picks the strongest backend that works here (bwrap, then podman, then docker).\n\
             # none is UNSAFE and only for the mock and replay providers. A real provider refuses it.\n\
             isolation = {isolation}\n\
             # Container image for docker and podman. It must exist locally, runs never pull.\n\
             image = {image}\n\
             # Limits for one check.\n\
             memory_mb = {memory}\n\
             cpus = {cpus:?}\n\
             max_processes = {procs}\n\
             max_file_mb = {file_mb}\n\
             # Extra host folders mounted read only, for example a toolchain.\n\
             ro_binds = {ro_binds}\n\
             # Layouts where model-written files could shadow hidden tests: warn, error or off.\n\
             # error refuses to run while there is any warning.\n\
             shadow_warnings = {shadow}\n\
             \n\
             [meta]\n\
             defaults_confirmed = {confirmed}\n",
            rules_file = toml_string(&self.project.rules_file),
            model = toml_string(&self.project.model),
            repeats = self.evaluation.repeats,
            ho_threshold = self.evaluation.held_out_accept_threshold_pp,
            ho_wins = self.evaluation.held_out_min_paired_wins,
            ho_large = self.evaluation.held_out_large_set_tasks,
            ho_large_threshold = self.evaluation.held_out_large_set_threshold_pp,
            threshold = self.evaluation.accept_threshold_pp,
            wins = self.evaluation.min_paired_wins,
            tokens = self.budget.max_tokens_per_run,
            max_change = self.mutation.max_change_bytes,
            retries = self.mutation.retries_on_refusal,
            one_shot = self.mutation.one_shot_agent,
            model_errors = self.evaluation.max_model_errors_per_run,
            isolation = toml_string(self.sandbox.isolation.as_str()),
            image = toml_string(&self.sandbox.image),
            memory = self.sandbox.memory_mb,
            cpus = self.sandbox.cpus,
            shadow = toml_string(self.sandbox.shadow_warnings.as_str()),
            procs = self.sandbox.max_processes,
            file_mb = self.sandbox.max_file_mb,
            ro_binds = toml::Value::Array(
                self.sandbox
                    .ro_binds
                    .iter()
                    .map(|p| toml::Value::String(p.clone()))
                    .collect()
            ),
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

    #[test]
    fn held_out_bar_depends_on_the_held_out_set_size() {
        let c = Config::with_defaults("AGENTS.md", "m");
        let e = &c.evaluation;
        assert_eq!(e.held_out_large_set_tasks, 20);
        assert_eq!(e.held_out_threshold_for(8), 10.0);
        assert_eq!(e.held_out_threshold_for(19), 10.0);
        assert_eq!(e.held_out_threshold_for(20), 5.0);
        assert_eq!(e.held_out_threshold_for(24), 5.0);
        let mut off = e.clone();
        off.held_out_large_set_tasks = 0;
        assert_eq!(off.held_out_threshold_for(100), 10.0);
        // the template carries both keys and reads back the same
        let text = c.to_toml_template();
        assert!(text.contains("held_out_large_set_tasks = 20"), "{text}");
        assert!(
            text.contains("held_out_large_set_threshold_pp = 5.0"),
            "{text}"
        );
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn an_old_config_without_the_large_set_keys_gets_the_defaults() {
        let text = Config::with_defaults("AGENTS.md", "m")
            .to_toml_template()
            .lines()
            .filter(|l| !l.starts_with("held_out_large_set"))
            .collect::<Vec<_>>()
            .join("\n");
        let c: Config = toml::from_str(&text).unwrap();
        assert_eq!(c.evaluation.held_out_large_set_tasks, 20);
        assert_eq!(c.evaluation.held_out_large_set_threshold_pp, 5.0);
    }
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
    fn old_configs_without_a_sandbox_table_still_load() {
        let cfg = Config::with_defaults("AGENTS.md", "m");
        let text = cfg.to_toml_template();
        let cut = text.find("[sandbox]").unwrap();
        let end = text.find("[meta]").unwrap();
        let old = format!("{}{}", &text[..cut], &text[end..]);
        let parsed: Config = toml::from_str(&old).unwrap();
        assert_eq!(parsed.sandbox, SandboxSettings::default());
    }

    #[test]
    fn sandbox_table_can_be_changed_and_round_trips() {
        let mut cfg = Config::with_defaults("AGENTS.md", "m");
        cfg.sandbox.isolation = crate::isolation::Isolation::Podman;
        cfg.sandbox.ro_binds = vec!["/opt/toolchain".into()];
        cfg.sandbox.cpus = 1.5;
        let parsed: Config = toml::from_str(&cfg.to_toml_template()).unwrap();
        assert_eq!(parsed, cfg);
    }

    #[test]
    fn template_handles_quotes_in_model_name() {
        let cfg = Config::with_defaults("AGENTS.md", "weird \"name\"");
        let parsed: Config = toml::from_str(&cfg.to_toml_template()).unwrap();
        assert_eq!(parsed.project.model, "weird \"name\"");
    }
}
