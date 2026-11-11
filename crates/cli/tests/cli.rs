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

// ---------------------------------------------------------------------------
// Phase 2: benchmark harness with the mock model, end to end.
// ---------------------------------------------------------------------------

const N_TASKS: usize = 12;

/// A tiny repo with 12 tasks. Task `tN` passes when `answers/tN.txt` holds `ok`.
/// `check.sh` is the hidden benchmark file. The mock model has a 30 percent
/// base pass rate, and three markers that change it.
fn bench_project(mock_seed: u64) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("AGENTS.md"), "base rules\n").unwrap();
    std::fs::write(root.join("check.sh"), "grep -qx ok \"answers/$1.txt\"\n").unwrap();
    let out = run(root, &["init", "--model", "mock-model", "--json"]);
    assert!(out.status.success());

    let mut tasks = String::from("[sandbox]\nhidden = [\"check.sh\"]\n");
    let mut mock = format!(
        "seed = {mock_seed}\nbase_pass_probability = 0.3\ntokens_per_call = 100\n\
         [[marker]]\ntext = \"GOOD\"\ndelta = 0.6\n\
         [[marker]]\ntext = \"BAD\"\ndelta = -0.3\n\
         [[marker]]\ntext = \"SLIGHT\"\ndelta = 0.1\n"
    );
    for i in 0..N_TASKS {
        tasks.push_str(&format!(
            "[[task]]\nid = \"t{i}\"\nprompt = \"solve task {i}\"\ncheck = \"sh check.sh t{i}\"\n"
        ));
        mock.push_str(&format!(
            "[[answer]]\ntask = \"t{i}\"\npath = \"answers/t{i}.txt\"\npass_content = \"ok\\n\"\nfail_content = \"no\\n\"\n"
        ));
    }
    std::fs::write(root.join(".rules-evolve/tasks.toml"), tasks).unwrap();
    std::fs::write(root.join(".rules-evolve/mock.toml"), mock).unwrap();
    dir
}

fn ok_json(dir: &Path, args: &[&str]) -> Value {
    let mut full = args.to_vec();
    full.push("--json");
    let out = run(dir, &full);
    assert!(
        out.status.code() == Some(0),
        "{args:?} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    json_of(&out)
}

/// Propose a child with the given rules text, run parent and child, verify.
/// Returns the verify `data`.
fn evolve(dir: &Path, text: &str, hypothesis: &str) -> Value {
    let file = dir.join("candidate.txt");
    std::fs::write(&file, text).unwrap();
    let p = ok_json(
        dir,
        &[
            "propose",
            "--file",
            file.to_str().unwrap(),
            "--hypothesis",
            hypothesis,
            "--parent",
            "root",
        ],
    );
    assert_eq!(p["data"]["outcome"], "proposed");
    let child = p["data"]["child"].as_str().unwrap().to_string();
    ok_json(dir, &["run", "--version", "root"]);
    ok_json(dir, &["run", "--version", &child]);
    ok_json(dir, &["verify", "--child", &child])["data"].clone()
}

#[test]
fn end_to_end_accepted_rejected_and_noise_with_the_mock_model() {
    // Mock seed 5 is a known case: the slight change wins every pair but
    // gains only 8.3 points, so it is noise.
    let dir = bench_project(5);
    let d = dir.path();

    let accepted = evolve(d, "base rules\nGOOD\n", "add the good hint");
    assert_eq!(accepted["decision"]["verdict"], "accepted", "{accepted}");
    assert!(accepted["decision"]["gain_pp"].as_f64().unwrap() >= 10.0);
    assert!(accepted["decision"]["paired_wins"].as_u64().unwrap() >= 4);

    let rejected = evolve(d, "base rules\nBAD\n", "add the bad hint");
    assert_eq!(rejected["decision"]["verdict"], "rejected", "{rejected}");
    assert!(
        rejected["decision"]["child_median"].as_f64().unwrap()
            <= rejected["decision"]["parent_median"].as_f64().unwrap()
    );

    let noise = evolve(d, "base rules\nSLIGHT\n", "add a slight hint");
    assert_eq!(noise["decision"]["verdict"], "noise", "{noise}");
    assert!(noise["decision"]["gain_pp"].as_f64().unwrap() < 10.0);
    assert!(noise["decision"]["gain_pp"].as_f64().unwrap() > 0.0);

    // The tree and the log tell the same story, and the chain is intact.
    let status = ok_json(d, &["status"]);
    let tree = &status["data"]["tree"];
    let nodes = tree["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 4);
    let verdicts: Vec<&str> = tree["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["verdict"].as_str().unwrap())
        .collect();
    assert_eq!(verdicts, ["accepted", "rejected", "noise"]);
    assert_eq!(tree["current"], accepted["child"]);
    assert_eq!(status["data"]["valid"], true);
    // 4 versions x 5 runs recorded.
    let runs = ok_json(d, &["log", "--kind", "run_recorded"]);
    assert_eq!(runs["data"]["total"], 20);
    let first = &runs["data"]["events"][0]["payload"];
    assert_eq!(first["total"], N_TASKS);
    assert_eq!(first["provider"], "mock");
    assert!(first["seed"].is_u64());
    assert!(first["tokens_used"].as_u64().unwrap() > 0);
    assert_eq!(first["tasks"].as_array().unwrap().len(), N_TASKS);
    let v = ok_json(d, &["verify-log"]);
    assert_eq!(v["data"]["valid"], true);
    // Check files and the real script were never touched by the model.
    assert_eq!(
        std::fs::read_to_string(d.join("check.sh")).unwrap(),
        "grep -qx ok \"answers/$1.txt\"\n"
    );
}

#[test]
fn run_json_shape_and_resume() {
    let dir = bench_project(5);
    let d = dir.path();
    let v = ok_json(d, &["run", "--version", "root", "--repeats", "2"]);
    assert_eq!(v["command"], "run");
    let data = &v["data"];
    assert_eq!(data["provider"], "mock");
    assert_eq!(data["repeats"], 2);
    assert_eq!(data["max_tokens_per_run"], 200000);
    let runs = data["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0]["total"], N_TASKS);
    assert!(runs[0]["pass_rate"].is_f64());
    assert_eq!(runs[0]["budget_exhausted"], false);
    assert!(runs[0]["tasks"][0]["status"].is_string());

    let v = ok_json(d, &["run", "--version", "root", "--repeats", "3"]);
    assert_eq!(v["data"]["skipped_existing"], serde_json::json!([0, 1]));
    assert_eq!(v["data"]["runs"].as_array().unwrap().len(), 1);
}

#[test]
fn token_cap_is_enforced_through_the_cli() {
    let dir = bench_project(5);
    let d = dir.path();
    let cfg = d.join(".rules-evolve/config.toml");
    let text = std::fs::read_to_string(&cfg).unwrap();
    // One call costs about 100 tokens plus a few for the text. Cap at 350.
    std::fs::write(&cfg, text.replace("200000", "350")).unwrap();
    let v = ok_json(d, &["run", "--version", "root", "--repeats", "1"]);
    let run0 = &v["data"]["runs"][0];
    assert!(run0["tokens_used"].as_u64().unwrap() <= 350);
    assert_eq!(run0["budget_exhausted"], true);
    let skipped = run0["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["status"] == "skipped_budget")
        .count();
    assert!(skipped >= N_TASKS - 3, "{skipped}");
}

#[test]
fn propose_size_limit_exit_code_and_log() {
    let dir = bench_project(5);
    let d = dir.path();
    let file = d.join("big.txt");
    std::fs::write(&file, format!("base rules\n{}", "x".repeat(5000))).unwrap();
    let out = run(
        d,
        &[
            "propose",
            "--file",
            file.to_str().unwrap(),
            "--hypothesis",
            "rewrite everything",
            "--json",
        ],
    );
    assert_eq!(out.status.code(), Some(3));
    let v = json_of(&out);
    assert_eq!(v["data"]["outcome"], "refused");
    assert!(v["data"]["child"].is_null());
    let log = ok_json(d, &["log", "--kind", "mutation_refused"]);
    assert_eq!(log["data"]["total"], 1);
    let status = ok_json(d, &["status"]);
    assert_eq!(
        status["data"]["tree"]["refused"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn verify_before_running_and_unknown_provider_are_errors() {
    let dir = bench_project(5);
    let d = dir.path();
    let file = d.join("c.txt");
    std::fs::write(&file, "base rules\nGOOD\n").unwrap();
    let p = ok_json(
        d,
        &[
            "propose",
            "--file",
            file.to_str().unwrap(),
            "--hypothesis",
            "h",
        ],
    );
    let child = p["data"]["child"].as_str().unwrap().to_string();
    let out = run(d, &["verify", "--child", &child, "--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(json_of(&out)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("missing"));
    let out = run(d, &["run", "--provider", "openai", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(json_of(&out)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("not available"));
}

#[test]
fn run_without_mock_settings_explains_what_to_do() {
    let dir = bench_project(5);
    std::fs::remove_file(dir.path().join(".rules-evolve/mock.toml")).unwrap();
    let out = run(dir.path(), &["run", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(json_of(&out)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("mock model settings"));
}
