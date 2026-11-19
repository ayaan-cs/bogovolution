//! Helpers for building small lineages in unit tests.

use crate::state::{ChainStatus, ConfigView, WatchState};
use bogovolution_core::event::{
    Event, InitPayload, MutationProposedPayload, MutationRefusedPayload, Payload,
    RunRecordedPayload, VerdictKind, VerdictPayload,
};
use bogovolution_core::log::Log;

pub fn h(c: char) -> String {
    c.to_string().repeat(64)
}

pub fn init() -> Payload {
    Payload::Init(InitPayload {
        rules_file: "AGENTS.md".into(),
        model: "demo-model".into(),
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
        seed: Some(u64::from(idx)),
        ..Default::default()
    })
}

/// Chain checked events for the payloads, with fixed timestamps.
pub fn events(payloads: &[Payload]) -> Vec<Event> {
    let dir = tempfile::tempdir().unwrap();
    let log = Log::new(dir.path());
    for p in payloads {
        log.append_at(p, "2026-01-01T00:00:00Z").unwrap();
    }
    log.read_events().unwrap()
}

pub fn build(payloads: &[Payload]) -> WatchState {
    let ev = events(payloads);
    WatchState::build(&ev, ChainStatus::ok(ev.len() as u64), ConfigView::default())
}

/// Root a, then b accepted, c rejected, d noise, three refusals, and e pending.
pub fn story() -> Vec<Payload> {
    vec![
        init(),
        run('a', 0, 3),
        proposed('a', 'b', "Run the test suite before finishing."),
        run('b', 0, 8),
        verdict('a', 'b', VerdictKind::Accepted, 0.3, 0.8, 5),
        proposed('b', 'c', "Skip the tests to move faster."),
        verdict('b', 'c', VerdictKind::Rejected, 0.8, 0.4, 0),
        proposed('b', 'd', "Prefer small commits."),
        verdict('b', 'd', VerdictKind::Noise, 0.8, 0.85, 3),
        refused(
            'b',
            Some("Explain everything at length."),
            "change of 1628 bytes is over the limit of 1000",
        ),
        refused(
            'b',
            Some("Prefer small commits."),
            "this content already exists in the tree as abc",
        ),
        refused('b', None, "the hypothesis is empty"),
        proposed('b', 'e', "Read the failing test first."),
    ]
}
