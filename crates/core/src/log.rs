//! The append-only, hash-chained JSON Lines log.
//!
//! One event per line. Each event stores the hash of the previous event, so
//! changing, removing or reordering any earlier line breaks the chain.
//! A small `HEAD` file next to the log records the newest `seq` and `hash`.
//! It lets `verify` notice when lines were cut off the end of the log.

use crate::error::{io_err, Error, Result};
use crate::event::{Event, Payload};
use crate::hash::GENESIS_HASH;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The `HEAD` file contents: where the chain should end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeadRecord {
    pub seq: u64,
    pub hash: String,
}

/// One thing wrong with the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Problem {
    /// Stable code: `invalid_line`, `bad_seq`, `bad_prev_hash`, `bad_hash`,
    /// `head_missing`, `head_mismatch`.
    pub code: String,
    /// Event sequence number the problem was found at, when known.
    pub seq: Option<u64>,
    /// 1-based line number in the log file, when known.
    pub line: Option<usize>,
    pub message: String,
}

/// Result of checking the chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifyReport {
    pub valid: bool,
    /// Number of events that passed the chain check.
    pub events: u64,
    pub head_seq: Option<u64>,
    pub head_hash: Option<String>,
    pub problems: Vec<Problem>,
}

/// Check log text (and optionally the head record) without touching disk.
/// Returns the report plus the events that passed the chain check.
pub fn verify_text(text: &str, head: Option<&HeadRecord>) -> (VerifyReport, Vec<Event>) {
    let mut events: Vec<Event> = Vec::new();
    let mut problems: Vec<Problem> = Vec::new();
    let mut prev = GENESIS_HASH.to_string();

    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        if raw.trim().is_empty() {
            problems.push(Problem {
                code: "invalid_line".into(),
                seq: None,
                line: Some(line_no),
                message: "blank line in log".into(),
            });
            break;
        }
        let event: Event = match serde_json::from_str(raw) {
            Ok(e) => e,
            Err(e) => {
                problems.push(Problem {
                    code: "invalid_line".into(),
                    seq: None,
                    line: Some(line_no),
                    message: format!("line is not a valid event (possibly cut off): {e}"),
                });
                break;
            }
        };
        let expected_seq = events.len() as u64;
        if event.seq != expected_seq {
            problems.push(Problem {
                code: "bad_seq".into(),
                seq: Some(event.seq),
                line: Some(line_no),
                message: format!("expected seq {expected_seq}, found {}", event.seq),
            });
            break;
        }
        if event.prev_hash != prev {
            problems.push(Problem {
                code: "bad_prev_hash".into(),
                seq: Some(event.seq),
                line: Some(line_no),
                message: "prev_hash does not match the previous event".into(),
            });
            break;
        }
        if event.hash != event.compute_hash() {
            problems.push(Problem {
                code: "bad_hash".into(),
                seq: Some(event.seq),
                line: Some(line_no),
                message: "hash does not match the event contents (event was edited)".into(),
            });
            break;
        }
        prev = event.hash.clone();
        events.push(event);
    }

    if problems.is_empty() {
        match (head, events.last()) {
            (None, Some(last)) => problems.push(Problem {
                code: "head_missing".into(),
                seq: Some(last.seq),
                line: None,
                message: "HEAD file is missing, so cut off lines cannot be ruled out".into(),
            }),
            (Some(h), Some(last)) if h.seq != last.seq || h.hash != last.hash => {
                problems.push(Problem {
                    code: "head_mismatch".into(),
                    seq: Some(last.seq),
                    line: None,
                    message: format!(
                        "HEAD says the log ends at seq {}, but it ends at seq {} (log was cut off or HEAD was edited)",
                        h.seq, last.seq
                    ),
                })
            }
            (Some(h), None) => problems.push(Problem {
                code: "head_mismatch".into(),
                seq: None,
                line: None,
                message: format!("HEAD says the log ends at seq {}, but the log is empty", h.seq),
            }),
            _ => {}
        }
    }

    let report = VerifyReport {
        valid: problems.is_empty(),
        events: events.len() as u64,
        head_seq: events.last().map(|e| e.seq),
        head_hash: events.last().map(|e| e.hash.clone()),
        problems,
    };
    (report, events)
}

fn now_timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// A log file on disk.
#[derive(Debug, Clone)]
pub struct Log {
    path: PathBuf,
    head_path: PathBuf,
}

impl Log {
    /// `dir` is the `.graft` directory. Files are `log.jsonl` and `HEAD`.
    pub fn new(dir: &Path) -> Log {
        Log {
            path: dir.join("log.jsonl"),
            head_path: dir.join("HEAD"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    fn read_text(&self) -> Result<String> {
        match std::fs::read_to_string(&self.path) {
            Ok(t) => Ok(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(io_err(&self.path)(e)),
        }
    }

    fn read_head(&self) -> Result<Option<HeadRecord>> {
        match std::fs::read_to_string(&self.head_path) {
            Ok(t) => Ok(serde_json::from_str(&t).ok()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_err(&self.head_path)(e)),
        }
    }

    /// Check the whole chain. Returns the report and the valid events.
    pub fn verify(&self) -> Result<(VerifyReport, Vec<Event>)> {
        let text = self.read_text()?;
        let head = self.read_head()?;
        Ok(verify_text(&text, head.as_ref()))
    }

    /// Repair the two marks a write cut off by a crash or a kill can leave
    /// (v2.7.2), and say what was done. Anything else is left alone, so
    /// tampering is still reported by `verify`.
    ///
    /// - The last line is incomplete (no newline at the end) and everything
    ///   before it, with HEAD, is valid: the incomplete bytes are moved to
    ///   `log.jsonl.partial` and the log ends at the last whole event.
    /// - The log has one whole event more than HEAD says, and HEAD matches the
    ///   event before it: the event was written but HEAD was not, so HEAD is
    ///   brought up to date.
    ///
    /// Only commands that write call this, never readers such as `watch`, so
    /// a line that is still being written is never touched.
    pub fn recover_interrupted_write(&self) -> Result<Option<String>> {
        let mut text = self.read_text()?;
        let head = self.read_head()?;
        let mut done: Option<String> = None;
        if !text.is_empty() && !text.ends_with('\n') {
            // A whole event cut off just before its newline: add the newline,
            // or the next event would be glued onto the same line.
            let cut = text.rfind('\n').map(|i| i + 1).unwrap_or(0);
            if serde_json::from_str::<Event>(&text[cut..]).is_ok() {
                let mut f = OpenOptions::new()
                    .append(true)
                    .open(&self.path)
                    .map_err(io_err(&self.path))?;
                f.write_all(b"\n").map_err(io_err(&self.path))?;
                f.sync_all().map_err(io_err(&self.path))?;
                text.push('\n');
                done = Some("the last event was missing its newline; it was added".to_string());
            }
        }
        if verify_text(&text, head.as_ref()).0.valid {
            return Ok(done);
        }
        if !text.is_empty() && !text.ends_with('\n') {
            let cut = text.rfind('\n').map(|i| i + 1).unwrap_or(0);
            let (whole, partial) = text.split_at(cut);
            let (report, _) = verify_text(whole, head.as_ref());
            let head_ok = match (&head, report.head_seq) {
                (None, None) => true,
                _ => report.valid,
            };
            if head_ok {
                let side = self.path.with_extension("jsonl.partial");
                let mut f = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&side)
                    .map_err(io_err(&side))?;
                f.write_all(partial.as_bytes()).map_err(io_err(&side))?;
                f.write_all(b"\n").map_err(io_err(&side))?;
                f.sync_all().map_err(io_err(&side))?;
                let log = OpenOptions::new()
                    .write(true)
                    .open(&self.path)
                    .map_err(io_err(&self.path))?;
                log.set_len(cut as u64).map_err(io_err(&self.path))?;
                log.sync_all().map_err(io_err(&self.path))?;
                return Ok(Some(format!(
                    "the last write to the log was cut off; {} incomplete bytes were moved to {}",
                    partial.len(),
                    side.display()
                )));
            }
            return Ok(done);
        }
        if let Some(h) = &head {
            let (report, events) = verify_text(&text, None);
            let chain_ok = report.problems.iter().all(|p| p.code == "head_missing");
            if chain_ok && events.len() >= 2 {
                let before = &events[events.len() - 2];
                if h.seq == before.seq && h.hash == before.hash {
                    let last = &events[events.len() - 1];
                    let fixed = HeadRecord {
                        seq: last.seq,
                        hash: last.hash.clone(),
                    };
                    let head_text = serde_json::to_string(&fixed).expect("head encodes");
                    std::fs::write(&self.head_path, head_text).map_err(io_err(&self.head_path))?;
                    let note = format!(
                        "HEAD was one event behind the log (seq {} written, HEAD not updated); HEAD now points at it",
                        last.seq
                    );
                    return Ok(Some(match done {
                        Some(d) => format!("{d}; {note}"),
                        None => note,
                    }));
                }
            }
        }
        Ok(done)
    }

    /// Read all events, failing if the chain is not valid.
    pub fn read_events(&self) -> Result<Vec<Event>> {
        let (report, events) = self.verify()?;
        if report.valid {
            Ok(events)
        } else {
            Err(Error::LogInvalid(
                report
                    .problems
                    .first()
                    .map(|p| p.message.clone())
                    .unwrap_or_default(),
            ))
        }
    }

    /// Append an event stamped with the current time.
    pub fn append(&self, payload: &Payload) -> Result<Event> {
        self.append_at(payload, &now_timestamp())
    }

    /// Append an event with an explicit timestamp (used by tests).
    /// Refuses to append to a log whose chain is not valid.
    pub fn append_at(&self, payload: &Payload, timestamp: &str) -> Result<Event> {
        let events = self.read_events()?;
        let (seq, prev) = match events.last() {
            Some(last) => (last.seq + 1, last.hash.clone()),
            None => (0, GENESIS_HASH.to_string()),
        };
        let event = Event::new(seq, timestamp, payload, prev);
        let mut line = serde_json::to_string(&event).expect("event encodes");
        line.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(io_err(&self.path))?;
        file.write_all(line.as_bytes())
            .map_err(io_err(&self.path))?;
        file.sync_all().map_err(io_err(&self.path))?;

        let head = HeadRecord {
            seq: event.seq,
            hash: event.hash.clone(),
        };
        let head_text = serde_json::to_string(&head).expect("head encodes");
        std::fs::write(&self.head_path, head_text).map_err(io_err(&self.head_path))?;
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{InitPayload, MutationRefusedPayload, Payload};
    use tempfile::tempdir;

    fn init_payload() -> Payload {
        Payload::Init(InitPayload {
            rules_file: "AGENTS.md".into(),
            model: "test-model".into(),
            root_hash: "a".repeat(64),
            tool_version: "0.1.0".into(),
        })
    }

    fn refused(reason: &str) -> Payload {
        Payload::MutationRefused(MutationRefusedPayload {
            parent: "a".repeat(64),
            hypothesis: None,
            reason: reason.into(),
            ..Default::default()
        })
    }

    fn build_log(n: usize) -> (tempfile::TempDir, Log) {
        let dir = tempdir().unwrap();
        let log = Log::new(dir.path());
        log.append_at(&init_payload(), "2026-01-01T00:00:00Z")
            .unwrap();
        for i in 1..n {
            log.append_at(&refused(&format!("r{i}")), "2026-01-01T00:00:01Z")
                .unwrap();
        }
        (dir, log)
    }

    #[test]
    fn valid_chain_verifies() {
        let (_d, log) = build_log(4);
        let (report, events) = log.verify().unwrap();
        assert!(report.valid, "{report:?}");
        assert_eq!(report.events, 4);
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].prev_hash, GENESIS_HASH);
        assert_eq!(events[1].prev_hash, events[0].hash);
        assert_eq!(report.head_seq, Some(3));
    }

    #[test]
    fn hash_is_stable_across_reads() {
        let (_d, log) = build_log(2);
        let events = log.read_events().unwrap();
        for e in &events {
            assert_eq!(e.hash, e.compute_hash());
        }
    }

    #[test]
    fn medians_that_are_not_short_decimals_still_verify() {
        // 10 of 24 is 0.4166666666666667, which only reads back to the
        // same f64 with serde_json's float_roundtrip feature (v2.7.1).
        let dir = tempdir().unwrap();
        let log = Log::new(dir.path());
        log.append_at(&init_payload(), "2026-01-01T00:00:00Z")
            .unwrap();
        let verdict = Payload::Verdict(crate::event::VerdictPayload {
            parent: "a".repeat(64),
            child: "b".repeat(64),
            verdict: crate::event::VerdictKind::Noise,
            parent_median: Some(9.0 / 24.0),
            child_median: Some(10.0 / 24.0),
            paired_wins: Some(3),
            repeats: Some(5),
            note: None,
            held_out_parent_median: Some(6.0 / 24.0),
            held_out_child_median: Some(5.0 / 24.0),
            held_out_paired_wins: Some(1),
        });
        log.append_at(&verdict, "2026-01-01T00:00:01Z").unwrap();
        log.append_at(&refused("after"), "2026-01-01T00:00:02Z")
            .unwrap();
        let (report, _) = log.verify().unwrap();
        assert!(report.valid, "{report:?}");
        assert_eq!(report.events, 3);
    }

    #[test]
    fn edited_event_is_detected() {
        let (_d, log) = build_log(3);
        let text = std::fs::read_to_string(log.path()).unwrap();
        let tampered = text.replace("r1", "xx");
        std::fs::write(log.path(), tampered).unwrap();
        let (report, _) = log.verify().unwrap();
        assert!(!report.valid);
        assert_eq!(report.problems[0].code, "bad_hash");
        assert_eq!(report.problems[0].seq, Some(1));
    }

    #[test]
    fn edited_event_with_fixed_hash_breaks_next_link() {
        let (_d, log) = build_log(3);
        let mut events = log.read_events().unwrap();
        events[1].payload["reason"] = serde_json::json!("changed");
        events[1].hash = events[1].compute_hash();
        let text: String = events
            .iter()
            .map(|e| serde_json::to_string(e).unwrap() + "\n")
            .collect();
        std::fs::write(log.path(), text).unwrap();
        let (report, _) = log.verify().unwrap();
        assert!(!report.valid);
        assert_eq!(report.problems[0].code, "bad_prev_hash");
        assert_eq!(report.problems[0].seq, Some(2));
    }

    #[test]
    fn removed_middle_event_is_detected() {
        let (_d, log) = build_log(4);
        let text = std::fs::read_to_string(log.path()).unwrap();
        let kept: Vec<&str> = text
            .lines()
            .enumerate()
            .filter(|(i, _)| *i != 1)
            .map(|(_, l)| l)
            .collect();
        std::fs::write(log.path(), kept.join("\n") + "\n").unwrap();
        let (report, _) = log.verify().unwrap();
        assert!(!report.valid);
        assert_eq!(report.problems[0].code, "bad_seq");
    }

    #[test]
    fn truncated_tail_is_detected_by_head() {
        let (_d, log) = build_log(4);
        let text = std::fs::read_to_string(log.path()).unwrap();
        let kept: Vec<&str> = text.lines().take(2).collect();
        std::fs::write(log.path(), kept.join("\n") + "\n").unwrap();
        let (report, events) = log.verify().unwrap();
        assert!(!report.valid);
        assert_eq!(report.problems[0].code, "head_mismatch");
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn half_written_last_line_is_detected() {
        let (_d, log) = build_log(3);
        let text = std::fs::read_to_string(log.path()).unwrap();
        let cut = &text[..text.len() - 30];
        std::fs::write(log.path(), cut).unwrap();
        let (report, _) = log.verify().unwrap();
        assert!(!report.valid);
        assert_eq!(report.problems[0].code, "invalid_line");
        assert_eq!(report.problems[0].line, Some(3));
    }

    #[test]
    fn missing_head_is_reported() {
        let (d, log) = build_log(2);
        std::fs::remove_file(d.path().join("HEAD")).unwrap();
        let (report, _) = log.verify().unwrap();
        assert!(!report.valid);
        assert_eq!(report.problems[0].code, "head_missing");
    }

    #[test]
    fn append_refuses_broken_log() {
        let (_d, log) = build_log(3);
        let text = std::fs::read_to_string(log.path()).unwrap();
        std::fs::write(log.path(), text.replace("r1", "xx")).unwrap();
        let err = log.append(&refused("more")).unwrap_err();
        assert!(matches!(err, Error::LogInvalid(_)));
    }

    #[test]
    fn empty_log_is_valid() {
        let dir = tempdir().unwrap();
        let log = Log::new(dir.path());
        let (report, events) = log.verify().unwrap();
        assert!(report.valid);
        assert!(events.is_empty());
    }
}
