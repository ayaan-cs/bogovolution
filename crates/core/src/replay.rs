//! Recording and replay of model calls.
//!
//! A replay file is JSON Lines. The first line is a header that names the
//! provider the answers came from. Every other line is one recorded call:
//!
//! ```text
//! {"type":"header","format":1,"provider":"mock"}
//! {"type":"entry","key":"...","kind":"task","model":"m","task_id":"t1","seed":7,
//!  "rules_hash":"...","prompt_sha256":"...","response":{...}}
//! ```
//!
//! The `key` is the SHA-256 of the canonical JSON of
//! `{kind, model, prompt_sha256, provider, rules_hash, seed, task_id}`, so the
//! same request always finds the same answer, and any change to the provider,
//! model name, prompt, rules or seed is a miss.
//!
//! [`RecordingModel`] wraps any provider and writes each new call into a file.
//! [`ReplayModel`] answers only from a file. It never calls anything, so it
//! costs nothing, and a request it has no answer for is a loud error.

use crate::error::{io_err, Error, Result};
use crate::hash::{canonical_json, sha256_hex};
use crate::isolation::ProviderKind;
use crate::model::{
    Counters, Model, ModelRequest, ModelResponse, MutationRequest, MutationResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Version of the replay file format.
pub const REPLAY_FORMAT: u32 = 1;

/// What kind of call an entry holds.
pub const KIND_TASK: &str = "task";
pub const KIND_MUTATION: &str = "mutation";

/// One line of a replay file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Line {
    Header {
        format: u32,
        provider: String,
    },
    Entry {
        key: String,
        kind: String,
        model: String,
        task_id: String,
        seed: u64,
        rules_hash: String,
        prompt_sha256: String,
        response: Value,
    },
}

/// The stable key for a call, and the prompt hash it was built from.
pub fn call_key(
    kind: &str,
    provider: &str,
    model: &str,
    prompt: &str,
    rules_hash: &str,
    task_id: &str,
    seed: u64,
) -> (String, String) {
    let prompt_sha256 = sha256_hex(prompt.as_bytes());
    let key = sha256_hex(
        canonical_json(&json!({
            "kind": kind,
            "model": model,
            "prompt_sha256": prompt_sha256,
            "provider": provider,
            "rules_hash": rules_hash,
            "seed": seed,
            "task_id": task_id,
        }))
        .as_bytes(),
    );
    (key, prompt_sha256)
}

fn short(s: &str) -> &str {
    &s[..s.len().min(12)]
}

/// A loaded replay file.
#[derive(Debug, Clone)]
pub struct ReplayFile {
    pub provider: String,
    entries: HashMap<String, Line>,
    /// Keys in file order.
    order: Vec<String>,
}

impl ReplayFile {
    pub fn load(path: &Path) -> Result<ReplayFile> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::Input(format!("replay file {} does not exist", path.display()))
            } else {
                io_err(path)(e)
            }
        })?;
        let bad = |line: usize, msg: String| Error::Parse {
            path: path.to_path_buf(),
            message: format!("line {line}: {msg}"),
        };
        let mut provider: Option<String> = None;
        let mut entries = HashMap::new();
        let mut order = Vec::new();
        for (idx, raw) in text.lines().enumerate() {
            let n = idx + 1;
            let line: Line = serde_json::from_str(raw).map_err(|e| bad(n, e.to_string()))?;
            match (&line, &provider) {
                (
                    Line::Header {
                        format,
                        provider: p,
                    },
                    None,
                ) if idx == 0 => {
                    if *format != REPLAY_FORMAT {
                        return Err(bad(
                            n,
                            format!("unsupported replay format {format}, expected {REPLAY_FORMAT}"),
                        ));
                    }
                    provider = Some(p.clone());
                }
                (Line::Header { .. }, _) => return Err(bad(n, "unexpected header".into())),
                (Line::Entry { .. }, None) => {
                    return Err(bad(n, "the file must start with a header line".into()))
                }
                (Line::Entry { key, .. }, Some(_)) => {
                    if entries.insert(key.clone(), line.clone()).is_some() {
                        return Err(bad(n, "duplicate key".into()));
                    }
                    order.push(key.clone());
                }
            }
        }
        let provider = provider.ok_or_else(|| Error::Parse {
            path: path.to_path_buf(),
            message: "the file is empty (no header line)".into(),
        })?;
        Ok(ReplayFile {
            provider,
            entries,
            order,
        })
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    fn get(&self, key: &str) -> Option<&Value> {
        match self.entries.get(key) {
            Some(Line::Entry { response, .. }) => Some(response),
            _ => None,
        }
    }
}

/// Wraps a provider and records every new call into a replay file.
pub struct RecordingModel {
    inner: Box<dyn Model>,
    path: PathBuf,
    known: RefCell<HashSet<String>>,
    added: Cell<usize>,
}

fn write_line(path: &Path, line: &Line) -> Result<()> {
    let mut text = serde_json::to_string(line).expect("line encodes");
    text.push('\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(io_err(path))?;
    file.write_all(text.as_bytes()).map_err(io_err(path))
}

impl RecordingModel {
    /// Start recording into `path`. An existing file must hold answers from the
    /// same provider, and new calls are added to it.
    pub fn new(inner: Box<dyn Model>, path: &Path) -> Result<RecordingModel> {
        let mut known = HashSet::new();
        if path.exists() {
            let existing = ReplayFile::load(path)?;
            if existing.provider != inner.name() {
                return Err(Error::Input(format!(
                    "{} holds answers from provider {:?}, not {:?}. Use a different --record file",
                    path.display(),
                    existing.provider,
                    inner.name()
                )));
            }
            known.extend(existing.order.iter().cloned());
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(io_err(parent))?;
            }
            write_line(
                path,
                &Line::Header {
                    format: REPLAY_FORMAT,
                    provider: inner.name(),
                },
            )?;
        }
        Ok(RecordingModel {
            inner,
            path: path.to_path_buf(),
            known: RefCell::new(known),
            added: Cell::new(0),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        kind: &str,
        model: &str,
        prompt: &str,
        rules_hash: &str,
        task_id: &str,
        seed: u64,
        response: Value,
    ) -> Result<()> {
        let (key, prompt_sha256) = call_key(
            kind,
            &self.inner.name(),
            model,
            prompt,
            rules_hash,
            task_id,
            seed,
        );
        if !self.known.borrow_mut().insert(key.clone()) {
            return Ok(());
        }
        write_line(
            &self.path,
            &Line::Entry {
                key,
                kind: kind.to_string(),
                model: model.to_string(),
                task_id: task_id.to_string(),
                seed,
                rules_hash: rules_hash.to_string(),
                prompt_sha256,
                response,
            },
        )?;
        self.added.set(self.added.get() + 1);
        Ok(())
    }
}

impl Model for RecordingModel {
    fn name(&self) -> String {
        self.inner.name()
    }

    fn kind(&self) -> ProviderKind {
        self.inner.kind()
    }

    fn estimate_tokens(&self, req: &ModelRequest) -> u64 {
        self.inner.estimate_tokens(req)
    }

    fn complete(&self, req: &ModelRequest) -> Result<ModelResponse> {
        let resp = self.inner.complete(req)?;
        self.record(
            KIND_TASK,
            req.model,
            req.prompt,
            req.rules_hash,
            req.task_id,
            req.seed,
            serde_json::to_value(&resp).expect("response encodes"),
        )?;
        Ok(resp)
    }

    fn estimate_mutation_tokens(&self, req: &MutationRequest) -> u64 {
        self.inner.estimate_mutation_tokens(req)
    }

    fn propose_mutation(&self, req: &MutationRequest) -> Result<MutationResponse> {
        let resp = self.inner.propose_mutation(req)?;
        self.record(
            KIND_MUTATION,
            req.model,
            req.prompt,
            req.parent_hash,
            "",
            req.seed,
            serde_json::to_value(&resp).expect("response encodes"),
        )?;
        Ok(resp)
    }

    fn counters(&self) -> Counters {
        Counters {
            recorded: self.added.get(),
            replayed: 0,
        }
    }
}

/// Answers only from a replay file. No network, no cost.
pub struct ReplayModel {
    file: ReplayFile,
    path: PathBuf,
    hits: Cell<usize>,
    pace: Duration,
}

impl ReplayModel {
    pub fn load(path: &Path) -> Result<ReplayModel> {
        Ok(ReplayModel {
            file: ReplayFile::load(path)?,
            path: path.to_path_buf(),
            hits: Cell::new(0),
            pace: Duration::ZERO,
        })
    }

    /// Wait this long before every answer, to imitate a slow model. Only for
    /// watching a replay unfold in `watch`. It changes nothing in the results.
    pub fn with_pace(mut self, pace: Duration) -> ReplayModel {
        self.pace = pace;
        self
    }

    fn wait(&self) {
        if !self.pace.is_zero() {
            std::thread::sleep(self.pace);
        }
    }

    /// Number of recorded calls in the file.
    pub fn len(&self) -> usize {
        self.file.len()
    }

    pub fn is_empty(&self) -> bool {
        self.file.is_empty()
    }

    fn key(
        &self,
        kind: &str,
        model: &str,
        prompt: &str,
        rules_hash: &str,
        task_id: &str,
        seed: u64,
    ) -> String {
        call_key(
            kind,
            &self.file.provider,
            model,
            prompt,
            rules_hash,
            task_id,
            seed,
        )
        .0
    }

    fn miss(&self, what: &str, rules_hash: &str, seed: u64) -> Error {
        Error::ReplayMiss(format!(
            "replay miss: {} has no recorded answer for {what} (rules {}, seed {seed}). \
             The file was recorded with a different model name, task set, prompt, rules or seed. \
             Record it again with --record",
            self.path.display(),
            short(rules_hash)
        ))
    }
}

impl Model for ReplayModel {
    fn name(&self) -> String {
        "replay".to_string()
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Local
    }

    /// The recorded token count, or 0 on a miss so that `complete` can report
    /// the miss.
    fn estimate_tokens(&self, req: &ModelRequest) -> u64 {
        let key = self.key(
            KIND_TASK,
            req.model,
            req.prompt,
            req.rules_hash,
            req.task_id,
            req.seed,
        );
        self.file
            .get(&key)
            .and_then(|v| v.get("tokens_used"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    }

    fn complete(&self, req: &ModelRequest) -> Result<ModelResponse> {
        let key = self.key(
            KIND_TASK,
            req.model,
            req.prompt,
            req.rules_hash,
            req.task_id,
            req.seed,
        );
        let value = self
            .file
            .get(&key)
            .ok_or_else(|| self.miss(&format!("task {}", req.task_id), req.rules_hash, req.seed))?;
        let resp: ModelResponse =
            serde_json::from_value(value.clone()).map_err(|e| Error::Parse {
                path: self.path.clone(),
                message: format!("entry for task {} is not a task response: {e}", req.task_id),
            })?;
        self.hits.set(self.hits.get() + 1);
        self.wait();
        Ok(resp)
    }

    fn estimate_mutation_tokens(&self, req: &MutationRequest) -> u64 {
        let key = self.key(
            KIND_MUTATION,
            req.model,
            req.prompt,
            req.parent_hash,
            "",
            req.seed,
        );
        self.file
            .get(&key)
            .and_then(|v| v.get("tokens_used"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    }

    fn propose_mutation(&self, req: &MutationRequest) -> Result<MutationResponse> {
        let key = self.key(
            KIND_MUTATION,
            req.model,
            req.prompt,
            req.parent_hash,
            "",
            req.seed,
        );
        let value = self
            .file
            .get(&key)
            .ok_or_else(|| self.miss("a mutation", req.parent_hash, req.seed))?;
        let resp: MutationResponse =
            serde_json::from_value(value.clone()).map_err(|e| Error::Parse {
                path: self.path.clone(),
                message: format!("entry is not a mutation response: {e}"),
            })?;
        self.hits.set(self.hits.get() + 1);
        self.wait();
        Ok(resp)
    }

    fn counters(&self) -> Counters {
        Counters {
            recorded: 0,
            replayed: self.hits.get(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FileEdit, MockConfig, MockModel};
    use tempfile::tempdir;

    fn mock() -> Box<dyn Model> {
        let cfg: MockConfig = toml::from_str(
            "seed = 1\nbase_pass_probability = 1.0\ntokens_per_call = 10\n\
             [[answer]]\ntask = \"t\"\npath = \"a.txt\"\npass_content = \"ok\"\nfail_content = \"no\"\n\
             [[mutation]]\nhypothesis = \"h\"\nappend = \"more\"\n",
        )
        .unwrap();
        Box::new(MockModel::new(cfg))
    }

    fn treq<'a>(seed: u64, dir: &'a Path) -> ModelRequest<'a> {
        ModelRequest {
            model: "m",
            task_id: "t",
            prompt: "p",
            rules: "rules",
            rules_hash: "rh",
            workdir: dir,
            seed,
            run_index: 0,
            max_tokens: 1_000_000,
        }
    }

    fn mreq(seed: u64) -> MutationRequest<'static> {
        MutationRequest {
            model: "m",
            prompt: "mp",
            parent_rules: "rules\n",
            parent_hash: "ph",
            seed,
            max_change_bytes: 1000,
            max_tokens: 1_000_000,
        }
    }

    #[test]
    fn key_is_stable_and_sensitive_to_every_part() {
        let k = |kind, prov, model, prompt, rh, task, seed| {
            call_key(kind, prov, model, prompt, rh, task, seed).0
        };
        let base = k("task", "mock", "m", "p", "rh", "t", 1);
        assert_eq!(base, k("task", "mock", "m", "p", "rh", "t", 1));
        assert_eq!(base.len(), 64);
        for other in [
            k("mutation", "mock", "m", "p", "rh", "t", 1),
            k("task", "other", "m", "p", "rh", "t", 1),
            k("task", "mock", "m2", "p", "rh", "t", 1),
            k("task", "mock", "m", "p2", "rh", "t", 1),
            k("task", "mock", "m", "p", "rh2", "t", 1),
            k("task", "mock", "m", "p", "rh", "t2", 1),
            k("task", "mock", "m", "p", "rh", "t", 2),
        ] {
            assert_ne!(base, other);
        }
    }

    #[test]
    fn record_then_replay_gives_the_same_answers() {
        let d = tempdir().unwrap();
        let path = d.path().join("r.jsonl");
        let rec = RecordingModel::new(mock(), &path).unwrap();
        let w = d.path();
        let a = rec.complete(&treq(3, w)).unwrap();
        let b = rec.complete(&treq(4, w)).unwrap();
        let m = rec.propose_mutation(&mreq(9)).unwrap();
        // Asking again does not write a second entry.
        rec.complete(&treq(3, w)).unwrap();
        assert_eq!(rec.counters().recorded, 3);
        assert_eq!(rec.name(), "mock");

        let rep = ReplayModel::load(&path).unwrap();
        assert_eq!(rep.len(), 3);
        assert_eq!(rep.name(), "replay");
        assert_eq!(rep.complete(&treq(3, w)).unwrap(), a);
        assert_eq!(rep.complete(&treq(4, w)).unwrap(), b);
        assert_eq!(rep.propose_mutation(&mreq(9)).unwrap(), m);
        assert_eq!(rep.estimate_tokens(&treq(3, w)), a.tokens_used);
        assert_eq!(rep.estimate_mutation_tokens(&mreq(9)), m.tokens_used);
        assert_eq!(rep.counters().replayed, 3);
        assert_eq!(
            a.edits,
            vec![FileEdit {
                path: "a.txt".into(),
                content: "ok".into()
            }]
        );
    }

    #[test]
    fn cache_miss_is_a_clear_error() {
        let d = tempdir().unwrap();
        let path = d.path().join("r.jsonl");
        let rec = RecordingModel::new(mock(), &path).unwrap();
        rec.complete(&treq(3, d.path())).unwrap();
        let rep = ReplayModel::load(&path).unwrap();
        let err = rep.complete(&treq(5, d.path())).unwrap_err();
        assert!(matches!(err, Error::ReplayMiss(_)));
        let text = err.to_string();
        assert!(text.contains("no recorded answer"), "{text}");
        assert!(text.contains("seed 5"), "{text}");
        assert!(text.contains("--record"), "{text}");
        assert_eq!(rep.estimate_tokens(&treq(5, d.path())), 0);
        assert!(matches!(
            rep.propose_mutation(&mreq(1)),
            Err(Error::ReplayMiss(_))
        ));
        assert_eq!(rep.counters().replayed, 0);
    }

    #[test]
    fn recording_appends_to_an_existing_file_of_the_same_provider() {
        let d = tempdir().unwrap();
        let path = d.path().join("r.jsonl");
        let rec = RecordingModel::new(mock(), &path).unwrap();
        rec.complete(&treq(1, d.path())).unwrap();
        drop(rec);
        let rec = RecordingModel::new(mock(), &path).unwrap();
        rec.complete(&treq(1, d.path())).unwrap();
        rec.complete(&treq(2, d.path())).unwrap();
        assert_eq!(rec.counters().recorded, 1);
        assert_eq!(ReplayFile::load(&path).unwrap().len(), 2);
    }

    #[test]
    fn recording_refuses_a_file_from_another_provider() {
        let d = tempdir().unwrap();
        let path = d.path().join("r.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"header\",\"format\":1,\"provider\":\"other\"}\n",
        )
        .unwrap();
        assert!(RecordingModel::new(mock(), &path).is_err());
    }

    #[test]
    fn bad_replay_files_are_rejected() {
        let d = tempdir().unwrap();
        let p = d.path().join("x.jsonl");
        assert!(ReplayFile::load(&p).is_err());
        let write = |s: &str| std::fs::write(&p, s).unwrap();
        write("");
        assert!(ReplayFile::load(&p).is_err());
        write("not json\n");
        assert!(ReplayFile::load(&p).is_err());
        write("{\"type\":\"header\",\"format\":99,\"provider\":\"mock\"}\n");
        assert!(ReplayFile::load(&p).is_err());
        let entry = "{\"type\":\"entry\",\"key\":\"k\",\"kind\":\"task\",\"model\":\"m\",\"task_id\":\"t\",\"seed\":1,\"rules_hash\":\"r\",\"prompt_sha256\":\"p\",\"response\":{}}\n";
        write(entry);
        assert!(ReplayFile::load(&p).is_err());
        write(&format!(
            "{{\"type\":\"header\",\"format\":1,\"provider\":\"mock\"}}\n{entry}{entry}"
        ));
        let err = ReplayFile::load(&p).unwrap_err().to_string();
        assert!(err.contains("duplicate key"), "{err}");
    }

    #[test]
    fn provider_name_is_part_of_the_key() {
        // A file recorded as "mock" is looked up under "mock", whatever the
        // wrapper is called, so replay finds what the mock recorded.
        let d = tempdir().unwrap();
        let path = d.path().join("r.jsonl");
        let rec = RecordingModel::new(mock(), &path).unwrap();
        rec.complete(&treq(1, d.path())).unwrap();
        let file = ReplayFile::load(&path).unwrap();
        assert_eq!(file.provider, "mock");
        let (key, _) = call_key("task", "mock", "m", "p", "rh", "t", 1);
        assert!(file.get(&key).is_some());
    }
}
