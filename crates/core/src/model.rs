//! The model interface and a deterministic mock.
//!
//! The harness only talks to the [`Model`] trait. Phase 2 has exactly one
//! implementation, [`MockModel`], which makes no network calls and costs
//! nothing. Real bring-your-own-key providers will implement the same trait in
//! a later phase.

use crate::error::{Error, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;

/// One file the model wants written, path relative to the working folder.
#[derive(Debug, Clone, PartialEq)]
pub struct FileEdit {
    pub path: String,
    pub content: String,
}

/// What the harness hands the model for one task attempt.
#[derive(Debug, Clone, Copy)]
pub struct ModelRequest<'a> {
    pub task_id: &'a str,
    pub prompt: &'a str,
    /// Content of the rules file version being tested.
    pub rules: &'a str,
    /// SHA-256 of `rules`.
    pub rules_hash: &'a str,
    /// A copy of the repo with the benchmark check files left out. This is the
    /// only part of the disk a model is meant to read.
    pub workdir: &'a Path,
    /// Seed for this run. The same for parent and child at the same run index.
    pub seed: u64,
    pub run_index: u32,
    /// Tokens left in this run's budget. The model must not use more.
    pub max_tokens: u64,
}

/// What the model gave back.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelResponse {
    pub edits: Vec<FileEdit>,
    pub tokens_used: u64,
}

/// A model provider.
pub trait Model {
    /// Short provider name recorded in the log, for example `mock`.
    fn name(&self) -> String;
    /// A cautious guess of the tokens one call would use. The harness skips
    /// the call when this would go over the budget.
    fn estimate_tokens(&self, prompt: &str, rules: &str) -> u64;
    fn complete(&self, request: &ModelRequest) -> Result<ModelResponse>;
}

/// A marker: when `text` appears in the rules, the pass probability moves by `delta`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub text: String,
    pub delta: f64,
}

/// What a right and a wrong answer look like for one task. The mock writes
/// `pass_content` or `fail_content` to `path`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub task: String,
    pub path: String,
    pub pass_content: String,
    pub fail_content: String,
}

/// Mock model settings, read from `.rules-evolve/mock.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MockConfig {
    /// Mixed into every random draw.
    #[serde(default)]
    pub seed: u64,
    /// Chance of a correct answer when no marker applies. 0 to 1.
    #[serde(default)]
    pub base_pass_probability: f64,
    /// Chance, 0 to 1, that a sample ignores the draw shared by parent and
    /// child and uses its own. 0 means parent and child are fully paired.
    #[serde(default)]
    pub independent_noise: f64,
    /// Tokens charged per call, before the size based part.
    #[serde(default = "default_tokens")]
    pub tokens_per_call: u64,
    #[serde(default)]
    pub marker: Vec<Marker>,
    #[serde(default)]
    pub answer: Vec<Answer>,
}

fn default_tokens() -> u64 {
    1000
}

impl MockConfig {
    pub fn load(path: &Path) -> Result<MockConfig> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::Input(format!(
                    "mock model settings not found at {}. Create it (see the README) or pass --mock-config",
                    path.display()
                ))
            } else {
                Error::Io {
                    path: path.to_path_buf(),
                    source: e,
                }
            }
        })?;
        let cfg: MockConfig = toml::from_str(&text).map_err(|e| Error::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        cfg.validate().map_err(|m| Error::Parse {
            path: path.to_path_buf(),
            message: m,
        })?;
        Ok(cfg)
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        if !(0.0..=1.0).contains(&self.base_pass_probability) {
            return Err("base_pass_probability must be from 0 to 1".into());
        }
        if !(0.0..=1.0).contains(&self.independent_noise) {
            return Err("independent_noise must be from 0 to 1".into());
        }
        for m in &self.marker {
            if m.text.is_empty() {
                return Err("a marker has empty text".into());
            }
            if !(-1.0..=1.0).contains(&m.delta) {
                return Err(format!("marker {:?} delta must be from -1 to 1", m.text));
            }
        }
        Ok(())
    }
}

/// Uniform number in `[0, 1)` from a list of labels. Same labels, same number,
/// on every machine.
fn unit(parts: &[&str]) -> f64 {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.len().to_le_bytes());
        h.update(p.as_bytes());
    }
    let d = h.finalize();
    let x = u64::from_be_bytes(d[..8].try_into().expect("8 bytes"));
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// A deterministic stand in for a real model.
///
/// For each task attempt it works out a pass probability from the rules text
/// (`base_pass_probability` plus the `delta` of every marker found), draws a
/// number from the seed, and answers correctly when the draw is under the
/// probability. Parent and child get the same draw for the same run and task
/// unless `independent_noise` says otherwise, so a better rules file wins
/// pair by pair.
#[derive(Debug, Clone)]
pub struct MockModel {
    config: MockConfig,
}

impl MockModel {
    pub fn new(config: MockConfig) -> MockModel {
        MockModel { config }
    }

    /// Pass probability for a rules text, clamped to 0 to 1.
    pub fn pass_probability(&self, rules: &str) -> f64 {
        let mut p = self.config.base_pass_probability;
        for m in &self.config.marker {
            if rules.contains(&m.text) {
                p += m.delta;
            }
        }
        p.clamp(0.0, 1.0)
    }

    /// Whether the mock answers correctly for this request.
    pub fn would_pass(&self, req: &ModelRequest) -> bool {
        let p = self.pass_probability(req.rules);
        let cfg_seed = self.config.seed.to_string();
        let seed = req.seed.to_string();
        let run = req.run_index.to_string();
        let shared = unit(&["shared", &cfg_seed, &seed, &run, req.task_id]);
        let coin = unit(&["coin", &cfg_seed, &seed, &run, req.task_id, req.rules_hash]);
        let u = if coin < self.config.independent_noise {
            unit(&["own", &cfg_seed, &seed, &run, req.task_id, req.rules_hash])
        } else {
            shared
        };
        u < p
    }

    fn tokens_for(&self, prompt: &str, rules: &str) -> u64 {
        self.config.tokens_per_call + ((prompt.len() + rules.len()) / 4) as u64
    }
}

impl Model for MockModel {
    fn name(&self) -> String {
        "mock".to_string()
    }

    fn estimate_tokens(&self, prompt: &str, rules: &str) -> u64 {
        self.tokens_for(prompt, rules)
    }

    fn complete(&self, req: &ModelRequest) -> Result<ModelResponse> {
        let tokens = self.tokens_for(req.prompt, req.rules);
        if tokens > req.max_tokens {
            return Err(Error::Input(format!(
                "mock call needs {tokens} tokens but only {} are left",
                req.max_tokens
            )));
        }
        let ok = self.would_pass(req);
        let edits = self
            .config
            .answer
            .iter()
            .filter(|a| a.task == req.task_id)
            .map(|a| FileEdit {
                path: a.path.clone(),
                content: if ok {
                    a.pass_content.clone()
                } else {
                    a.fail_content.clone()
                },
            })
            .collect();
        Ok(ModelResponse {
            edits,
            tokens_used: tokens,
        })
    }
}

/// Build a provider by name. Only `mock` exists in Phase 2.
pub fn build_provider(name: &str, mock_config: &Path) -> Result<Box<dyn Model>> {
    match name {
        "mock" => Ok(Box::new(MockModel::new(MockConfig::load(mock_config)?))),
        other => Err(Error::Input(format!(
            "provider {other:?} is not available. Only \"mock\" exists so far; real providers come in a later phase"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(base: f64) -> MockConfig {
        MockConfig {
            seed: 1,
            base_pass_probability: base,
            independent_noise: 0.0,
            tokens_per_call: 100,
            marker: vec![
                Marker {
                    text: "GOOD".into(),
                    delta: 0.5,
                },
                Marker {
                    text: "BAD".into(),
                    delta: -0.5,
                },
            ],
            answer: vec![Answer {
                task: "t".into(),
                path: "out.txt".into(),
                pass_content: "ok".into(),
                fail_content: "no".into(),
            }],
        }
    }

    fn req<'a>(
        rules: &'a str,
        hash: &'a str,
        task: &'a str,
        run: u32,
        dir: &'a Path,
    ) -> ModelRequest<'a> {
        ModelRequest {
            task_id: task,
            prompt: "do it",
            rules,
            rules_hash: hash,
            workdir: dir,
            seed: 7,
            run_index: run,
            max_tokens: 1_000_000,
        }
    }

    fn rate(m: &MockModel, rules: &str, hash: &str) -> f64 {
        let dir = Path::new(".");
        let n = 400;
        let wins = (0..n)
            .filter(|i| m.would_pass(&req(rules, hash, &format!("task{i}"), 0, dir)))
            .count();
        wins as f64 / n as f64
    }

    #[test]
    fn same_inputs_same_answer() {
        let m = MockModel::new(cfg(0.5));
        let dir = Path::new(".");
        let a = m.complete(&req("x", "h", "t", 3, dir)).unwrap();
        let b = m.complete(&req("x", "h", "t", 3, dir)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn markers_move_the_pass_rate() {
        let m = MockModel::new(cfg(0.4));
        assert!((m.pass_probability("plain") - 0.4).abs() < 1e-12);
        assert!((m.pass_probability("has GOOD") - 0.9).abs() < 1e-12);
        assert_eq!(m.pass_probability("has BAD"), 0.0);
        assert!((m.pass_probability("GOOD BAD") - 0.4).abs() < 1e-12);
        let base = rate(&m, "plain", "h1");
        let good = rate(&m, "has GOOD", "h2");
        let bad = rate(&m, "has BAD", "h3");
        assert!(good > base + 0.3, "{good} vs {base}");
        assert!(bad < 0.01, "{bad}");
        assert!((base - 0.4).abs() < 0.1, "{base}");
    }

    #[test]
    fn extremes_are_exact() {
        let m = MockModel::new(cfg(0.0));
        assert_eq!(rate(&m, "x", "h"), 0.0);
        let m = MockModel::new(cfg(1.0));
        assert_eq!(rate(&m, "x", "h"), 1.0);
    }

    #[test]
    fn paired_draws_are_shared_without_noise() {
        // A child that only adds a positive marker never loses where the parent wins.
        let m = MockModel::new(cfg(0.3));
        let dir = Path::new(".");
        for i in 0..200 {
            let task = format!("task{i}");
            let parent = m.would_pass(&req("plain", "h1", &task, 2, dir));
            let child = m.would_pass(&req("plain GOOD", "h2", &task, 2, dir));
            assert!(child || !parent);
        }
    }

    #[test]
    fn noise_makes_versions_differ() {
        let mut c = cfg(0.5);
        c.independent_noise = 1.0;
        let m = MockModel::new(c);
        let dir = Path::new(".");
        let differing = (0..200)
            .filter(|i| {
                let t = format!("task{i}");
                m.would_pass(&req("a", "h1", &t, 0, dir))
                    != m.would_pass(&req("a", "h2", &t, 0, dir))
            })
            .count();
        assert!(differing > 40, "{differing}");
    }

    #[test]
    fn answers_follow_the_draw_and_tokens_are_charged() {
        let dir = Path::new(".");
        let m = MockModel::new(cfg(1.0));
        let r = m.complete(&req("rules text", "h", "t", 0, dir)).unwrap();
        assert_eq!(r.edits.len(), 1);
        assert_eq!(r.edits[0].content, "ok");
        assert_eq!(r.tokens_used, m.estimate_tokens("do it", "rules text"));
        let m = MockModel::new(cfg(0.0));
        assert_eq!(
            m.complete(&req("r", "h", "t", 0, dir)).unwrap().edits[0].content,
            "no"
        );
        // A task with no answer entry produces no edits.
        assert!(m
            .complete(&req("r", "h", "other", 0, dir))
            .unwrap()
            .edits
            .is_empty());
    }

    #[test]
    fn mock_refuses_to_exceed_max_tokens() {
        let dir = Path::new(".");
        let m = MockModel::new(cfg(1.0));
        let mut r = req("r", "h", "t", 0, dir);
        r.max_tokens = 10;
        assert!(m.complete(&r).is_err());
    }

    #[test]
    fn config_validation_and_unknown_provider() {
        let mut c = cfg(0.5);
        c.base_pass_probability = 1.5;
        assert!(c.validate().is_err());
        let mut c = cfg(0.5);
        c.marker[0].delta = 2.0;
        assert!(c.validate().is_err());
        assert!(build_provider("openai", Path::new("nope.toml")).is_err());
        assert!(build_provider("mock", Path::new("/nonexistent/mock.toml")).is_err());
    }

    #[test]
    fn mock_config_parses_from_toml() {
        let c: MockConfig = toml::from_str(
            "seed = 3\nbase_pass_probability = 0.2\n[[marker]]\ntext = \"a\"\ndelta = 0.1\n[[answer]]\ntask = \"t\"\npath = \"p\"\npass_content = \"y\"\nfail_content = \"n\"\n",
        )
        .unwrap();
        assert_eq!(c.seed, 3);
        assert_eq!(c.tokens_per_call, 1000);
        assert_eq!(c.marker.len(), 1);
        assert!(c.validate().is_ok());
    }
}
