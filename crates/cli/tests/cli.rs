//! Integration tests: run the real binary.

use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rules-evolve"))
}

fn run(dir: &Path, args: &[&str]) -> Output {
    bin().arg("-C").arg(dir).args(args).output().unwrap()
}

fn json_of(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("bad json {e}: {}", String::from_utf8_lossy(&out.stdout)))
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("AGENTS.md"), "# Rules\nBe careful.\n").unwrap();
    let out = run(dir.path(), &["init", "--model", "demo-model", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    dir
}

#[test]
fn init_then_status_then_log_then_verify() {
    let dir = project();

    let out = run(dir.path(), &["status", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let v = json_of(&out);
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "status");
    assert_eq!(v["data"]["valid"], true);
    assert_eq!(v["data"]["tree"]["rules_file"], "AGENTS.md");
    assert_eq!(v["data"]["tree"]["model"], "demo-model");
    assert_eq!(v["data"]["tree"]["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(v["data"]["config"]["evaluation"]["repeats"], 5);
    assert_eq!(v["data"]["config"]["meta"]["defaults_confirmed"], true);

    let out = run(dir.path(), &["log", "--json"]);
    let v = json_of(&out);
    assert_eq!(v["data"]["total"], 1);
    let ev = &v["data"]["events"][0];
    assert_eq!(ev["kind"], "init");
    assert_eq!(ev["seq"], 0);
    assert_eq!(ev["prev_hash"], "0".repeat(64));
    assert_eq!(ev["hash"].as_str().unwrap().len(), 64);

    let out = run(dir.path(), &["verify-log", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(json_of(&out)["data"]["valid"], true);

    // Human text mode works too.
    let out = run(dir.path(), &["status"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("intact"));
    assert!(!text.contains("not confirmed"));
}

#[test]
fn init_writes_expected_files_and_template() {
    let dir = project();
    let root = dir.path().join(".rules-evolve");
    for f in ["config.toml", "tasks.toml", "log.jsonl", "HEAD"] {
        assert!(root.join(f).is_file(), "{f} missing");
    }
    assert_eq!(std::fs::read_dir(root.join("blobs")).unwrap().count(), 1);
    let cfg = std::fs::read_to_string(root.join("config.toml")).unwrap();
    assert!(cfg.contains("repeats = 5"));
    assert!(cfg.contains("max_tokens_per_run = 200000"));
    assert!(cfg.contains("defaults_confirmed = true"));
}

#[test]
fn tampering_is_caught_with_exit_code_2() {
    let dir = project();
    let log = dir.path().join(".rules-evolve/log.jsonl");
    let text = std::fs::read_to_string(&log).unwrap();
    std::fs::write(&log, text.replace("demo-model", "other-model")).unwrap();

    let out = run(dir.path(), &["verify-log", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    let v = json_of(&out);
    assert_eq!(v["data"]["valid"], false);
    assert_eq!(v["data"]["chain"]["problems"][0]["code"], "bad_hash");

    let out = run(dir.path(), &["status", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(json_of(&out)["data"]["tree"].is_null());

    let out = run(dir.path(), &["log", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(json_of(&out)["ok"], false);
}

#[test]
fn errors_are_json_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["status", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let v = json_of(&out);
    assert_eq!(v["ok"], false);
    assert!(v["error"]["message"].as_str().unwrap().contains("init"));

    let out = run(dir.path(), &["init", "--model", "m", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(json_of(&out)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("no rules file"));
}

#[test]
fn log_filters_and_rejects_bad_kind() {
    let dir = project();
    let out = run(dir.path(), &["log", "--kind", "verdict", "--json"]);
    assert_eq!(json_of(&out)["data"]["shown"], 0);
    let out = run(dir.path(), &["log", "--limit", "1", "--json"]);
    assert_eq!(json_of(&out)["data"]["shown"], 1);
    let out = run(dir.path(), &["log", "--kind", "bogus", "--json"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn second_init_is_refused() {
    let dir = project();
    let out = run(dir.path(), &["init", "--model", "m", "--json"]);
    assert_eq!(out.status.code(), Some(1));
}
