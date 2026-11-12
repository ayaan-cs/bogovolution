//! Log events and their typed payloads.

use crate::hash::{canonical_json, sha256_hex};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// What kind of thing happened. Names are stable in the log file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Init,
    MutationProposed,
    MutationRefused,
    RunRecorded,
    Verdict,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Init => "init",
            EventKind::MutationProposed => "mutation_proposed",
            EventKind::MutationRefused => "mutation_refused",
            EventKind::RunRecorded => "run_recorded",
            EventKind::Verdict => "verdict",
        }
    }

    pub fn parse(s: &str) -> Option<EventKind> {
        match s {
            "init" => Some(EventKind::Init),
            "mutation_proposed" => Some(EventKind::MutationProposed),
            "mutation_refused" => Some(EventKind::MutationRefused),
            "run_recorded" => Some(EventKind::RunRecorded),
            "verdict" => Some(EventKind::Verdict),
            _ => None,
        }
    }
}

/// The outcome of comparing a child to its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictKind {
    Accepted,
    Rejected,
    Noise,
}

impl VerdictKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VerdictKind::Accepted => "accepted",
            VerdictKind::Rejected => "rejected",
            VerdictKind::Noise => "noise",
        }
    }
}

/// The first event in every log. Records the starting rules file version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitPayload {
    /// Rules file path, relative to the repo root, with forward slashes.
    pub rules_file: String,
    /// Model name. Only recorded, never called in Phase 1.
    pub model: String,
    /// Content hash of the starting rules file (the root node).
    pub root_hash: String,
    /// Version of the tool that wrote the log.
    pub tool_version: String,
}

/// A child version of a parent version, with a plain-English hypothesis.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MutationProposedPayload {
    pub parent: String,
    pub child: String,
    pub hypothesis: String,
    /// Seed the model was given. `None` for changes added by `propose`.
    #[serde(default)]
    pub seed: Option<u64>,
    /// Provider that wrote the change, for example `mock`.
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub tokens_used: Option<u64>,
}

/// A mutation that was not allowed (for example it was too large).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MutationRefusedPayload {
    pub parent: String,
    #[serde(default)]
    pub hypothesis: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub tokens_used: Option<u64>,
}

/// How one task attempt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// The check command exited with 0.
    Passed,
    /// The check command exited with a non-zero code.
    Failed,
    /// The check command ran past its timeout and was killed.
    Timeout,
    /// The model tried to write a path it may not touch. Counts as failed.
    Violation,
    /// The model call returned an error. Counts as failed.
    ModelError,
    /// Not attempted because the token cap would have been passed. Counts as failed.
    SkippedBudget,
    /// The model reported more tokens than were left, so its answer was thrown
    /// away. Counts as failed.
    OverBudget,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Passed => "passed",
            TaskStatus::Failed => "failed",
            TaskStatus::Timeout => "timeout",
            TaskStatus::Violation => "violation",
            TaskStatus::ModelError => "model_error",
            TaskStatus::SkippedBudget => "skipped_budget",
            TaskStatus::OverBudget => "over_budget",
        }
    }
}

/// Result of one task inside one run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskOutcome {
    pub id: String,
    pub status: TaskStatus,
    pub tokens_used: u64,
    /// A short note on what went wrong (files written, end of the check
    /// output with hidden file lines removed). Only on failed attempts, and
    /// missing in logs written before v2.2.0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// One benchmark run of one rules version over the whole task set.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RunRecordedPayload {
    pub node: String,
    pub run_index: u32,
    pub passed: u32,
    pub total: u32,
    #[serde(default)]
    pub tokens_used: u64,
    /// Seed of this run. Parent and child runs with the same index share it.
    #[serde(default)]
    pub seed: Option<u64>,
    /// Which model provider ran it, for example `mock`.
    #[serde(default)]
    pub provider: Option<String>,
    /// True when the run stopped early to stay under the token cap.
    #[serde(default)]
    pub budget_exhausted: bool,
    /// Per task results. Empty for runs recorded without detail.
    #[serde(default)]
    pub tasks: Vec<TaskOutcome>,
    /// Isolation backend the checks ran with (`bwrap`, `docker`, `podman` or
    /// `none`). Missing in logs written before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,
}

/// The final decision about a child, by the fixed public rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerdictPayload {
    pub parent: String,
    pub child: String,
    pub verdict: VerdictKind,
    #[serde(default)]
    pub parent_median: Option<f64>,
    #[serde(default)]
    pub child_median: Option<f64>,
    #[serde(default)]
    pub paired_wins: Option<u32>,
    #[serde(default)]
    pub repeats: Option<u32>,
    #[serde(default)]
    pub note: Option<String>,
}

/// Typed payload for each event kind.
#[derive(Debug, Clone, PartialEq)]
pub enum Payload {
    Init(InitPayload),
    MutationProposed(MutationProposedPayload),
    MutationRefused(MutationRefusedPayload),
    RunRecorded(RunRecordedPayload),
    Verdict(VerdictPayload),
}

impl Payload {
    pub fn kind(&self) -> EventKind {
        match self {
            Payload::Init(_) => EventKind::Init,
            Payload::MutationProposed(_) => EventKind::MutationProposed,
            Payload::MutationRefused(_) => EventKind::MutationRefused,
            Payload::RunRecorded(_) => EventKind::RunRecorded,
            Payload::Verdict(_) => EventKind::Verdict,
        }
    }

    pub fn to_value(&self) -> Value {
        let result = match self {
            Payload::Init(p) => serde_json::to_value(p),
            Payload::MutationProposed(p) => serde_json::to_value(p),
            Payload::MutationRefused(p) => serde_json::to_value(p),
            Payload::RunRecorded(p) => serde_json::to_value(p),
            Payload::Verdict(p) => serde_json::to_value(p),
        };
        result.expect("payload structs always encode")
    }

    /// Decode the payload of an event according to its kind.
    pub fn from_event(event: &Event) -> Result<Payload, serde_json::Error> {
        let v = event.payload.clone();
        Ok(match event.kind {
            EventKind::Init => Payload::Init(serde_json::from_value(v)?),
            EventKind::MutationProposed => Payload::MutationProposed(serde_json::from_value(v)?),
            EventKind::MutationRefused => Payload::MutationRefused(serde_json::from_value(v)?),
            EventKind::RunRecorded => Payload::RunRecorded(serde_json::from_value(v)?),
            EventKind::Verdict => Payload::Verdict(serde_json::from_value(v)?),
        })
    }
}

/// One line of the log file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    /// UTC time, RFC 3339, whole seconds, for example `2026-11-11T06:00:00Z`.
    pub timestamp: String,
    pub kind: EventKind,
    pub payload: Value,
    pub prev_hash: String,
    pub hash: String,
}

impl Event {
    /// Build an event and compute its hash.
    pub fn new(
        seq: u64,
        timestamp: impl Into<String>,
        payload: &Payload,
        prev_hash: impl Into<String>,
    ) -> Event {
        let mut event = Event {
            seq,
            timestamp: timestamp.into(),
            kind: payload.kind(),
            payload: payload.to_value(),
            prev_hash: prev_hash.into(),
            hash: String::new(),
        };
        event.hash = event.compute_hash();
        event
    }

    /// SHA-256 over the canonical JSON of every field except `hash`.
    /// Because `prev_hash` is included, each event commits to all earlier ones.
    pub fn compute_hash(&self) -> String {
        let body = json!({
            "seq": self.seq,
            "timestamp": self.timestamp,
            "kind": self.kind.as_str(),
            "payload": self.payload,
            "prev_hash": self.prev_hash,
        });
        sha256_hex(canonical_json(&body).as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_events_from_before_isolation_existed_still_read() {
        let old = serde_json::json!({
            "node": "a",
            "run_index": 0,
            "passed": 3,
            "total": 4
        });
        let p: RunRecordedPayload = serde_json::from_value(old).unwrap();
        assert_eq!(p.isolation, None);
    }

    #[test]
    fn a_missing_isolation_is_not_written_so_old_lines_keep_their_bytes() {
        let p = RunRecordedPayload::default();
        let v = serde_json::to_value(&p).unwrap();
        assert!(v.get("isolation").is_none());
        let p = RunRecordedPayload {
            isolation: Some("bwrap".into()),
            ..Default::default()
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["isolation"], "bwrap");
    }
}
