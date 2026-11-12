//! Helpers for building small trees in unit tests.

use crate::event::{
    InitPayload, MutationProposedPayload, MutationRefusedPayload, Payload, RunRecordedPayload,
    VerdictKind, VerdictPayload,
};
use crate::log::Log;
use crate::tree::Tree;
use tempfile::tempdir;

pub fn h(c: char) -> String {
    c.to_string().repeat(64)
}

pub fn init() -> Payload {
    Payload::Init(InitPayload {
        rules_file: "AGENTS.md".into(),
        model: "m".into(),
        root_hash: h('a'),
        tool_version: "0.1.0".into(),
    })
}

pub fn proposed(parent: char, child: char, hyp: &str) -> Payload {
    Payload::MutationProposed(MutationProposedPayload {
        parent: h(parent),
        child: h(child),
        hypothesis: hyp.into(),
        ..Default::default()
    })
}

pub fn refused(parent: char, hyp: Option<&str>, reason: &str) -> Payload {
    Payload::MutationRefused(MutationRefusedPayload {
        parent: h(parent),
        hypothesis: hyp.map(|s| s.to_string()),
        reason: reason.into(),
        ..Default::default()
    })
}

pub fn verdict(parent: char, child: char, v: VerdictKind, pm: f64, cm: f64, wins: u32) -> Payload {
    Payload::Verdict(VerdictPayload {
        parent: h(parent),
        child: h(child),
        verdict: v,
        parent_median: Some(pm),
        child_median: Some(cm),
        paired_wins: Some(wins),
        repeats: Some(5),
        note: Some(format!("because {}", v.as_str())),
        held_out_parent_median: None,
        held_out_child_median: None,
        held_out_paired_wins: None,
    })
}

pub fn run(node: char, idx: u32, passed: u32) -> Payload {
    Payload::RunRecorded(RunRecordedPayload {
        node: h(node),
        run_index: idx,
        passed,
        total: 10,
        tokens_used: 10,
        seed: Some(idx as u64),
        ..Default::default()
    })
}

pub fn tree(payloads: &[Payload]) -> Tree {
    let dir = tempdir().unwrap();
    let log = Log::new(dir.path());
    for p in payloads {
        log.append_at(p, "2026-01-01T00:00:00Z").unwrap();
    }
    Tree::from_events(&log.read_events().unwrap()).unwrap()
}
