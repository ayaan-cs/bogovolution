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
fn a_capped_run_says_why_and_stops_the_version() {
    let dir = bench_project(5);
    let d = dir.path();
    let cfg = d.join(".rules-evolve/config.toml");
    let text = std::fs::read_to_string(&cfg).unwrap();
    std::fs::write(&cfg, text.replace("200000", "350")).unwrap();
    let out = run(d, &["run", "--version", "root", "--repeats", "3"]);
    assert!(out.status.success());
    let words = String::from_utf8_lossy(&out.stdout);
    assert!(words.contains("STOPPED AT THE TOKEN CAP"), "{words}");
    assert!(words.contains("budget.max_tokens_per_run"), "{words}");
    // only the capped run was recorded, not three
    let runs = ok_json(d, &["log", "--kind", "run_recorded"]);
    assert_eq!(runs["data"]["total"], 1);
}

#[test]
fn a_half_written_log_line_is_repaired_and_reported_by_the_next_run() {
    let dir = bench_project(5);
    let d = dir.path();
    ok_json(d, &["run", "--version", "root", "--repeats", "1"]);
    let log = d.join(".rules-evolve/log.jsonl");
    let mut text = std::fs::read_to_string(&log).unwrap();
    text.push_str("{\"seq\":9,\"kind\":\"run_rec");
    std::fs::write(&log, text).unwrap();
    let out = run(d, &["run", "--version", "root", "--repeats", "2"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let words = String::from_utf8_lossy(&out.stdout);
    assert!(words.contains("Log repaired"), "{words}");
    assert!(d.join(".rules-evolve/log.jsonl.partial").exists());
    assert_eq!(ok_json(d, &["verify-log"])["data"]["valid"], true);
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
    let out = run(d, &["run", "--provider", "gemini", "--json"]);
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

// ---------------------------------------------------------------------------
// Phase 3: mutate, evolve, record and replay.
// ---------------------------------------------------------------------------

/// Copy the demo fixture into a new temp repo and run `init`.
fn demo_project() -> tempfile::TempDir {
    let demo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo");
    let dir = tempfile::tempdir().unwrap();
    for f in ["AGENTS.md", "check.sh"] {
        std::fs::copy(demo.join("fixture").join(f), dir.path().join(f)).unwrap();
    }
    let out = run(dir.path(), &["init", "--model", "demo-model", "--json"]);
    assert!(out.status.success());
    for f in ["tasks.toml", "mock.toml"] {
        std::fs::copy(demo.join(f), dir.path().join(".rules-evolve").join(f)).unwrap();
    }
    dir
}

/// The log without timestamps and without the provider name, so a mock run and
/// a replay of it can be compared.
fn outcome_log(dir: &Path) -> Vec<(String, Value)> {
    let v = ok_json(dir, &["log"]);
    v["data"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            let mut payload = e["payload"].clone();
            if let Some(map) = payload.as_object_mut() {
                map.remove("provider");
            }
            (e["kind"].as_str().unwrap().to_string(), payload)
        })
        .collect()
}

#[test]
fn mutate_through_the_cli_records_proposals_and_refusals() {
    let dir = bench_project(5);
    let d = dir.path();
    // Three candidates: good, empty hypothesis, no change.
    let mock = d.join(".rules-evolve/mock.toml");
    let mut text = std::fs::read_to_string(&mock).unwrap();
    text.push_str(
        "\n[[mutation]]\nhypothesis = \"add the good hint\"\nappend = \"GOOD\"\n\
         \n[[mutation]]\nhypothesis = \"\"\nappend = \"something\"\n\
         \n[[mutation]]\nhypothesis = \"do nothing\"\n",
    );
    std::fs::write(&mock, text).unwrap();

    let mut seen = std::collections::BTreeMap::new();
    for seed in 0..30 {
        let out = run(
            d,
            &[
                "mutate",
                "--parent",
                "root",
                "--seed",
                &seed.to_string(),
                "--json",
            ],
        );
        let v = json_of(&out);
        assert_eq!(v["command"], "mutate");
        let data = &v["data"];
        assert_eq!(data["provider"], "mock");
        assert_eq!(data["seed"], seed);
        assert!(data["tokens_used"].as_u64().unwrap() >= 100);
        match data["outcome"].as_str().unwrap() {
            "proposed" => {
                assert_eq!(out.status.code(), Some(0));
                assert_eq!(data["hypothesis"], "add the good hint");
                assert_eq!(data["child"].as_str().unwrap().len(), 64);
            }
            "refused" => {
                assert_eq!(out.status.code(), Some(3));
                assert!(data["child"].is_null());
            }
            other => panic!("{other}"),
        }
        *seen
            .entry(data["reason"].as_str().unwrap_or("proposed").to_string())
            .or_insert(0) += 1;
    }
    // Every kind of outcome showed up.
    assert!(seen.contains_key("proposed"), "{seen:?}");
    assert!(
        seen.keys().any(|r| r.contains("hypothesis is empty")),
        "{seen:?}"
    );
    assert!(
        seen.keys().any(|r| r.contains("identical to the parent")),
        "{seen:?}"
    );
    // The good change was added once, later picks of it were duplicates.
    assert_eq!(seen["proposed"], 1);
    assert!(
        seen.keys().any(|r| r.contains("already exists")),
        "{seen:?}"
    );

    let status = ok_json(d, &["status"]);
    assert_eq!(status["data"]["tree"]["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(status["data"]["valid"], true);
    let log = ok_json(d, &["log", "--kind", "mutation_proposed"]);
    let p = &log["data"]["events"][0]["payload"];
    assert_eq!(p["provider"], "mock");
    assert!(p["seed"].is_u64());
    assert!(p["tokens_used"].is_u64());
}

#[test]
fn mutate_is_deterministic_for_a_seed() {
    let make = || {
        let dir = demo_project();
        let out = run(
            dir.path(),
            &["mutate", "--parent", "root", "--seed", "42", "--json"],
        );
        let v = json_of(&out);
        let d = &v["data"];
        (
            d["outcome"].clone(),
            d["hypothesis"].clone(),
            d["child"].clone(),
            d["tokens_used"].clone(),
        )
    };
    assert_eq!(make(), make());
}

#[test]
fn record_then_replay_gives_identical_outcomes() {
    let a = demo_project();
    let b = demo_project();
    let replay = a.path().join("tmp-replay.jsonl");
    let rec = ok_json(
        a.path(),
        &[
            "evolve",
            "--generations",
            "3",
            "--seed",
            "7",
            "--record",
            replay.to_str().unwrap(),
        ],
    );
    assert!(rec["data"]["provider_stats"]["recorded"].as_u64().unwrap() > 0);
    assert_eq!(rec["data"]["provider_stats"]["replayed"], 0);

    let rep = ok_json(
        b.path(),
        &[
            "evolve",
            "--generations",
            "3",
            "--seed",
            "7",
            "--provider",
            "replay",
            "--replay-file",
            replay.to_str().unwrap(),
        ],
    );
    assert_eq!(rep["data"]["provider"], "replay");
    assert_eq!(rep["data"]["provider_stats"]["recorded"], 0);
    assert_eq!(
        rep["data"]["provider_stats"]["replayed"],
        rec["data"]["provider_stats"]["recorded"]
    );
    for k in [
        "accepted",
        "rejected",
        "noise",
        "refused",
        "end",
        "total_tokens",
    ] {
        assert_eq!(rec["data"][k], rep["data"][k], "{k}");
    }
    assert_eq!(outcome_log(a.path()), outcome_log(b.path()));
    assert_eq!(ok_json(b.path(), &["verify-log"])["data"]["valid"], true);

    // Recording the same calls again adds nothing new.
    let c = demo_project();
    let again = ok_json(
        c.path(),
        &[
            "run",
            "--version",
            "root",
            "--seed",
            "7",
            "--record",
            replay.to_str().unwrap(),
        ],
    );
    assert_eq!(again["data"]["provider_stats"]["recorded"], 0);
}

#[test]
fn replay_cache_miss_fails_clearly_and_records_nothing() {
    let a = demo_project();
    let replay = a.path().join("tmp-replay.jsonl");
    ok_json(
        a.path(),
        &[
            "run",
            "--version",
            "root",
            "--repeats",
            "1",
            "--seed",
            "1",
            "--record",
            replay.to_str().unwrap(),
        ],
    );
    let b = demo_project();
    let before = ok_json(b.path(), &["log"])["data"]["total"].clone();
    // A different seed was never recorded.
    let out = run(
        b.path(),
        &[
            "run",
            "--version",
            "root",
            "--repeats",
            "1",
            "--seed",
            "2",
            "--provider",
            "replay",
            "--replay-file",
            replay.to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let v = json_of(&out);
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(msg.contains("replay miss"), "{msg}");
    assert!(msg.contains("no recorded answer"), "{msg}");
    assert_eq!(ok_json(b.path(), &["log"])["data"]["total"], before);

    // Mutations miss too.
    let out = run(
        b.path(),
        &[
            "mutate",
            "--provider",
            "replay",
            "--replay-file",
            replay.to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(json_of(&out)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("replay miss"));
}

#[test]
fn replay_option_errors() {
    let dir = demo_project();
    let d = dir.path();
    let err = |args: &[&str]| {
        let mut full = args.to_vec();
        full.push("--json");
        let out = run(d, &full);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        json_of(&out)["error"]["message"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert!(err(&["run", "--provider", "replay"]).contains("--replay-file"));
    assert!(
        err(&["run", "--provider", "replay", "--replay-file", "nope.jsonl"])
            .contains("does not exist")
    );
    assert!(err(&[
        "run",
        "--provider",
        "replay",
        "--replay-file",
        "x.jsonl",
        "--record",
        "y.jsonl"
    ])
    .contains("--record cannot be used"));
    assert!(err(&["run", "--provider", "gemini"]).contains("not available"));
    assert!(err(&["run", "--provider", "openai"]).contains("--allow-spend"));
    // A broken replay file is reported with its line number.
    std::fs::write(d.join("bad.jsonl"), "not json\n").unwrap();
    assert!(err(&["run", "--provider", "replay", "--replay-file", "bad.jsonl"]).contains("line 1"));
}

#[test]
fn evolve_rejects_bad_generation_counts() {
    let dir = demo_project();
    for n in ["0", "101"] {
        let out = run(dir.path(), &["evolve", "--generations", n]);
        assert!(!out.status.success(), "{n}");
    }
}

#[test]
fn evolve_total_counts_generations_already_in_the_log() {
    let dir = demo_project();
    let d = dir.path();
    let a = ok_json(
        d,
        &["evolve", "--generations", "2", "--seed", "3", "--total"],
    );
    assert_eq!(a["data"]["generations"].as_array().unwrap().len(), 2);
    // Again: the total of 2 is reached, nothing more is done.
    let b = ok_json(
        d,
        &["evolve", "--generations", "2", "--seed", "3", "--total"],
    );
    assert_eq!(b["data"]["generations"].as_array().unwrap().len(), 0);
    assert_eq!(b["data"]["generations_already_done"], 2);
    // A total of 3 adds one, numbered 3.
    let c = ok_json(
        d,
        &["evolve", "--generations", "3", "--seed", "3", "--total"],
    );
    let gens = c["data"]["generations"].as_array().unwrap();
    assert_eq!(gens.len(), 1);
    assert_eq!(gens[0]["generation"], 3);
    let text = run(
        d,
        &["evolve", "--generations", "3", "--seed", "3", "--total"],
    );
    let out = String::from_utf8_lossy(&text.stdout);
    assert!(
        out.contains("Total 3: 3 generations were already in the log."),
        "{out}"
    );
}

/// The demo from the README, end to end, with no key and no cost.
#[test]
fn demo_replay_shows_accepted_rejected_noise_and_refused() {
    let demo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo");
    let replay = demo.join("replay.jsonl");
    let dir = demo_project();
    let d = dir.path();
    let v = ok_json(
        d,
        &[
            "evolve",
            "--generations",
            "10",
            "--seed",
            "7",
            "--provider",
            "replay",
            "--replay-file",
            replay.to_str().unwrap(),
        ],
    );
    let data = &v["data"];
    assert_eq!(data["provider"], "replay");
    assert_eq!(data["provider_stats"]["recorded"], 0);
    assert!(data["provider_stats"]["replayed"].as_u64().unwrap() > 500);
    let counts = (
        data["accepted"].as_u64().unwrap(),
        data["rejected"].as_u64().unwrap(),
        data["noise"].as_u64().unwrap(),
        data["refused"].as_u64().unwrap(),
    );
    assert_eq!(counts, (2, 5, 1, 2));
    assert_eq!(data["generations"].as_array().unwrap().len(), 10);
    assert_ne!(data["start"], data["end"]);

    let status = ok_json(d, &["status"]);
    assert_eq!(status["data"]["valid"], true);
    let tree = &status["data"]["tree"];
    assert_eq!(tree["current"], data["end"]);
    assert_eq!(tree["nodes"].as_array().unwrap().len(), 9);
    assert_eq!(ok_json(d, &["verify-log"])["data"]["valid"], true);

    // The hidden benchmark file in the fixture was never changed.
    assert!(std::fs::read_to_string(d.join("check.sh"))
        .unwrap()
        .contains("grep -qx ok"));
}

/// The committed replay is exactly what the mock would record today. If this
/// fails, run `examples/demo/demo.sh --record` and commit the new file.
#[test]
fn committed_demo_replay_matches_a_fresh_mock_recording() {
    let demo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo");
    let dir = demo_project();
    let fresh = dir.path().join("fresh-replay.jsonl");
    let rec = ok_json(
        dir.path(),
        &[
            "evolve",
            "--generations",
            "10",
            "--seed",
            "7",
            "--record",
            fresh.to_str().unwrap(),
        ],
    );
    assert_eq!(
        std::fs::read_to_string(&fresh).unwrap(),
        std::fs::read_to_string(demo.join("replay.jsonl")).unwrap(),
        "examples/demo/replay.jsonl is out of date"
    );
    // And replaying the committed file gives the same lineage as the mock.
    let other = demo_project();
    ok_json(
        other.path(),
        &[
            "evolve",
            "--generations",
            "10",
            "--seed",
            "7",
            "--provider",
            "replay",
            "--replay-file",
            demo.join("replay.jsonl").to_str().unwrap(),
        ],
    );
    assert_eq!(outcome_log(dir.path()), outcome_log(other.path()));
    assert_eq!(rec["data"]["accepted"], 2);
}

// ---------------------------------------------------------------------
// Phase 4: digest and export
// ---------------------------------------------------------------------

fn demo_replay_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/replay.jsonl")
}

/// The demo project with the committed replay already played.
fn evolved_demo() -> tempfile::TempDir {
    let dir = demo_project();
    let replay = demo_replay_path();
    ok_json(
        dir.path(),
        &[
            "evolve",
            "--generations",
            "10",
            "--seed",
            "7",
            "--provider",
            "replay",
            "--replay-file",
            replay.to_str().unwrap(),
        ],
    );
    dir
}

fn schema_validator() -> jsonschema::Validator {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/export.v1.json");
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    jsonschema::validator_for(&schema).expect("schema compiles")
}

fn assert_valid(v: &jsonschema::Validator, doc: &Value) {
    let errors: Vec<String> = v
        .iter_errors(doc)
        .map(|e| format!("{} at {}", e, e.instance_path))
        .collect();
    assert!(errors.is_empty(), "schema errors: {errors:#?}");
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn export_of_the_demo_validates_against_the_published_schema() {
    let dir = evolved_demo();
    let out = dir.path().join("out").join("tree.json");
    let v = ok_json(dir.path(), &["export", "--out", out.to_str().unwrap()]);
    assert_eq!(v["command"], "export");
    assert_eq!(v["data"]["nodes"], 9);
    assert_eq!(v["data"]["edges"], 8);
    assert_eq!(v["data"]["refused"], 2);

    let doc = read_json(&out);
    assert_valid(&schema_validator(), &doc);
    assert_eq!(doc["format"], "rules-evolve-export");
    assert_eq!(doc["version"], 1);
    assert_eq!(doc["nodes"].as_array().unwrap().len(), 9);
    assert_eq!(doc["edges"].as_array().unwrap().len(), 8);
    assert_eq!(doc["refused"].as_array().unwrap().len(), 2);
    assert_eq!(doc["series"].as_array().unwrap().len(), 10);
    assert_eq!(doc["config"]["accept_threshold_pp"], 10.0);
    assert_eq!(doc["config"]["min_paired_wins"], 4);
    assert_eq!(doc["config"]["max_change_bytes"], 1000);
    assert_eq!(doc["summary"]["accepted"], 2);

    // The chain head in the export is the last event in the log.
    let log = ok_json(dir.path(), &["log", "--limit", "1"]);
    let last = &log["data"]["events"][0];
    assert_eq!(doc["chain"]["head_hash"], last["hash"]);
    assert_eq!(doc["chain"]["head_seq"], last["seq"]);
    assert_eq!(doc["chain"]["events"], log["data"]["total"]);

    // Root has no diff, every other version has a git style one.
    let nodes = doc["nodes"].as_array().unwrap();
    assert_eq!(nodes[0]["status"], "root");
    assert!(nodes[0]["diff"].is_null());
    for n in &nodes[1..] {
        assert!(n["diff"]
            .as_str()
            .unwrap()
            .starts_with("diff --git a/AGENTS.md"));
    }
    let current: Vec<&Value> = nodes.iter().filter(|n| n["is_current"] == true).collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0]["hash"], doc["current"]);

    // Every edge carries its run pairs, and a rejected edge carries a reason.
    let edges = doc["edges"].as_array().unwrap();
    assert!(edges
        .iter()
        .all(|e| e["pairs"].as_array().unwrap().len() == 5));
    assert!(
        edges
            .iter()
            .any(|e| e["verdict"] == "rejected"
                && e["reason"].as_str().unwrap().contains("not above"))
    );
}

#[test]
fn export_is_deterministic_for_one_log() {
    let dir = evolved_demo();
    let a = dir.path().join("a.json");
    let b = dir.path().join("b.json");
    ok_json(dir.path(), &["export", "--out", a.to_str().unwrap()]);
    ok_json(dir.path(), &["export", "--out", b.to_str().unwrap()]);
    assert_eq!(
        std::fs::read_to_string(a).unwrap(),
        std::fs::read_to_string(b).unwrap()
    );
}

#[test]
fn export_to_stdout_prints_the_bare_document() {
    let dir = evolved_demo();
    let out = run(dir.path(), &["export", "--out", "-"]);
    assert!(out.status.success());
    let doc = json_of(&out);
    assert_valid(&schema_validator(), &doc);
    let wrapped = ok_json(dir.path(), &["export", "--out", "-"]);
    assert_eq!(wrapped["data"], doc);
}

#[test]
fn export_of_a_fresh_or_pending_project_validates() {
    let v = schema_validator();
    let dir = project();
    let out = dir.path().join("fresh.json");
    ok_json(dir.path(), &["export", "--out", out.to_str().unwrap()]);
    let doc = read_json(&out);
    assert_valid(&v, &doc);
    assert_eq!(doc["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(doc["series"].as_array().unwrap().len(), 0);
    assert_eq!(doc["root"], doc["current"]);

    // A proposed but never judged change is pending, with no pairs.
    let new_file = dir.path().join("new.md");
    std::fs::write(&new_file, "# Rules\nBe careful.\nRead first.\n").unwrap();
    ok_json(
        dir.path(),
        &[
            "propose",
            "--file",
            new_file.to_str().unwrap(),
            "--hypothesis",
            "Reading first helps.",
        ],
    );
    ok_json(dir.path(), &["export", "--out", out.to_str().unwrap()]);
    let doc = read_json(&out);
    assert_valid(&v, &doc);
    assert_eq!(doc["nodes"][1]["status"], "pending");
    assert_eq!(doc["series"][0]["outcome"], "pending");
    assert!(doc["edges"][0]["gain_pp"].is_null());
    assert_eq!(doc["edges"][0]["pairs"].as_array().unwrap().len(), 0);
}

#[test]
fn the_schema_rejects_documents_that_drift_from_it() {
    let dir = evolved_demo();
    let out = dir.path().join("t.json");
    ok_json(dir.path(), &["export", "--out", out.to_str().unwrap()]);
    let good = read_json(&out);
    let v = schema_validator();
    assert_valid(&v, &good);

    let mut wrong_version = good.clone();
    wrong_version["version"] = serde_json::json!(2);
    assert!(!v.is_valid(&wrong_version));

    let mut missing = good.clone();
    missing.as_object_mut().unwrap().remove("nodes");
    assert!(!v.is_valid(&missing));

    let mut extra = good.clone();
    extra["surprise"] = serde_json::json!(true);
    assert!(!v.is_valid(&extra));

    let mut bad_hash = good.clone();
    bad_hash["root"] = serde_json::json!("not-a-hash");
    assert!(!v.is_valid(&bad_hash));

    let mut bad_verdict = good.clone();
    bad_verdict["edges"][0]["verdict"] = serde_json::json!("great");
    assert!(!v.is_valid(&bad_verdict));
}

#[test]
fn committed_demo_export_validates_and_matches_a_fresh_one() {
    let committed =
        read_json(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/tree.json"));
    assert_valid(&schema_validator(), &committed);
    let bundled =
        read_json(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../viewer/src/demo/tree.json"));
    assert_eq!(committed, bundled, "run examples/demo/export.sh");

    // Same lineage as a fresh replay. Only the chain hashes depend on the
    // log's timestamps, so they are not compared.
    let dir = evolved_demo();
    let out = dir.path().join("fresh.json");
    ok_json(dir.path(), &["export", "--out", out.to_str().unwrap()]);
    let mut fresh = read_json(&out);
    let mut old = committed;
    for doc in [&mut fresh, &mut old] {
        doc.as_object_mut().unwrap().remove("chain");
    }
    // Hashes of versions come from content only, so everything else matches.
    assert_eq!(
        fresh, old,
        "run examples/demo/export.sh and commit the result"
    );
}

#[test]
fn export_and_digest_refuse_a_tampered_log() {
    let dir = evolved_demo();
    let log = dir.path().join(".rules-evolve").join("log.jsonl");
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("\"demo-model\""));
    std::fs::write(&log, text.replacen("\"demo-model\"", "\"evil-model\"", 1)).unwrap();

    let out = dir.path().join("t.json");
    let r = run(
        dir.path(),
        &["export", "--out", out.to_str().unwrap(), "--json"],
    );
    assert_eq!(r.status.code(), Some(1));
    assert!(!out.exists(), "nothing is written for a bad chain");
    assert!(json_of(&r)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("verification"));

    let r = run(dir.path(), &["digest"]);
    assert_eq!(r.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&r.stderr).contains("verification"));
}

#[test]
fn digest_text_and_json_describe_the_demo() {
    let dir = evolved_demo();
    let text = run(dir.path(), &["digest"]);
    assert!(text.status.success());
    let md = String::from_utf8(text.stdout).unwrap();
    assert!(md.starts_with("# Rules lineage digest"));
    assert!(md.contains("2 accepted, 5 rejected, 1 noise, 0 pending. 2 refused"));
    assert!(md.contains("+33.3 points, 5 of 5 paired wins"));
    assert!(md.contains("Skipping tests will make the agent faster"));
    assert!(md.contains("this content already exists in the tree"));
    assert!(md.contains("3 of 5 paired wins"));
    assert!(md.contains("## Directions not tried yet"));
    assert!(!md.contains('\u{2014}') && !md.contains('\u{2013}'));

    // Same text on every run.
    let again = run(dir.path(), &["digest"]);
    assert_eq!(md.as_bytes(), again.stdout.as_slice());

    let v = ok_json(dir.path(), &["digest"]);
    assert_eq!(v["command"], "digest");
    let d = &v["data"];
    assert_eq!(d["worked"].as_array().unwrap().len(), 2);
    assert_eq!(d["rejected"].as_array().unwrap().len(), 5);
    assert_eq!(d["noise"].as_array().unwrap().len(), 1);
    assert_eq!(d["pending"].as_array().unwrap().len(), 0);
    assert_eq!(d["refused"].as_array().unwrap().len(), 2);
    assert!(!d["suggestions"].as_array().unwrap().is_empty());
    assert_eq!(d["progress"]["root_pass_rate"], 0.25);

    // The committed sample is what the demo produces.
    let sample = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/digest.md"),
    )
    .unwrap();
    assert_eq!(sample, md, "run examples/demo/export.sh");
}

#[test]
fn digest_of_a_fresh_project_suggests_starting() {
    let dir = project();
    let v = ok_json(dir.path(), &["digest"]);
    assert_eq!(v["data"]["suggestions"][0]["kind"], "start");
    assert_eq!(v["data"]["worked"].as_array().unwrap().len(), 0);
}

// ---------------------------------------------------------------------
// v1.1: watch and replay pace
// ---------------------------------------------------------------------

fn repo_file(rel: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

#[test]
fn watch_snapshot_of_the_finished_demo_matches_the_committed_text() {
    let dir = evolved_demo();
    let out = run(
        dir.path(),
        &[
            "watch",
            "--snapshot",
            "--width",
            "130",
            "--height",
            "34",
            "--select",
            "current",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    for want in [
        "2 accepted  5 rejected  1 noise  0 pending  2 refused",
        "chain OK  64 events",
        "ACCEPTED  +33 pts  5/5",
        "Mutation tree (* is current)",
        "Run progress",
        "Verdict feed",
    ] {
        assert!(text.contains(want), "missing {want:?}\n{text}");
    }
    assert!(!text.contains('\u{2014}') && !text.contains('\u{2013}'));
    let committed = std::fs::read_to_string(repo_file("docs/screenshots/watch-demo.txt")).unwrap();
    assert_eq!(
        text, committed,
        "run examples/demo/watch-snapshot.sh and commit the result"
    );
}

#[test]
fn watch_snapshot_svg_matches_the_committed_picture() {
    let dir = evolved_demo();
    let svg = dir.path().join("w.svg");
    let out = run(
        dir.path(),
        &[
            "watch",
            "--snapshot",
            "--width",
            "130",
            "--height",
            "34",
            "--select",
            "current",
            "--svg",
            svg.to_str().unwrap(),
        ],
    );
    assert!(out.status.success());
    let doc = std::fs::read_to_string(&svg).unwrap();
    assert!(doc.starts_with("<svg ") && doc.trim_end().ends_with("</svg>"));
    assert!(!doc.contains("<script"));
    assert_eq!(
        doc,
        std::fs::read_to_string(repo_file("docs/screenshots/watch-demo.svg")).unwrap(),
        "run examples/demo/watch-snapshot.sh and commit the result"
    );
}

#[test]
fn watch_snapshot_has_a_json_form_and_never_writes_the_log() {
    let dir = evolved_demo();
    let log = dir.path().join(".rules-evolve").join("log.jsonl");
    let before = std::fs::read(&log).unwrap();
    let v = ok_json(dir.path(), &["watch", "--snapshot"]);
    assert_eq!(v["command"], "watch");
    assert_eq!(v["data"]["width"], 110);
    assert!(v["data"]["text"].as_str().unwrap().contains("chain OK"));
    assert_eq!(std::fs::read(&log).unwrap(), before);
}

#[test]
fn watch_snapshot_shows_a_broken_chain_clearly() {
    let dir = evolved_demo();
    let log = dir.path().join(".rules-evolve").join("log.jsonl");
    let text = std::fs::read_to_string(&log).unwrap();
    // Tamper somewhere in the middle of the log, by editing a hypothesis.
    let tampered = text.replacen("Asking for small commits", "Asking for tiny commits", 1);
    assert_ne!(tampered, text);
    std::fs::write(&log, &tampered).unwrap();
    let out = run(dir.path(), &["watch", "--snapshot"]);
    assert!(out.status.success());
    let shown = String::from_utf8(out.stdout).unwrap();
    assert!(shown.contains("CHAIN FAILED VERIFICATION"), "{shown}");
    assert!(shown.contains("CHAIN INVALID"));
    assert!(shown.contains("read only"));
    assert_eq!(std::fs::read_to_string(&log).unwrap(), tampered);
}

#[test]
fn watch_needs_a_terminal_and_checks_run_text_first() {
    let dir = evolved_demo();
    let out = run(dir.path(), &["watch"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("needs a terminal"));

    let out = run(dir.path(), &["watch", "--run", "status"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--run can start one of"));

    let out = run(dir.path(), &["watch", "--run", "evolve -C /tmp"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("-C"));

    let out = run(dir.path(), &["watch", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(json_of(&out)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("interactive"));

    // Snapshot options only make sense with --snapshot.
    assert_eq!(
        run(dir.path(), &["watch", "--width", "80"]).status.code(),
        Some(2)
    );

    // Not initialized.
    let empty = tempfile::tempdir().unwrap();
    let out = run(empty.path(), &["watch", "--snapshot"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn pace_slows_a_replay_but_changes_nothing_else() {
    let replay = demo_replay_path();
    let fast = demo_project();
    let slow = demo_project();
    let args = |extra: &'static [&'static str]| {
        let mut a = vec![
            "evolve",
            "--generations",
            "1",
            "--seed",
            "7",
            "--provider",
            "replay",
            "--replay-file",
            replay.to_str().unwrap(),
        ];
        a.extend_from_slice(extra);
        a
    };
    let a = ok_json(fast.path(), &args(&[]));
    let started = std::time::Instant::now();
    let b = ok_json(slow.path(), &args(&["--pace", "2"]));
    assert!(started.elapsed() >= std::time::Duration::from_millis(40));
    assert_eq!(a["data"]["end"], b["data"]["end"]);
    assert_eq!(a["data"]["total_tokens"], b["data"]["total_tokens"]);
    assert_eq!(outcome_log(fast.path()), outcome_log(slow.path()));
}

#[test]
fn pace_is_refused_for_the_mock_provider() {
    let dir = demo_project();
    let out = run(
        dir.path(),
        &["evolve", "--generations", "1", "--pace", "5", "--json"],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(json_of(&out)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("--pace only works with --provider replay"));
}

#[test]
fn the_core_crate_has_no_terminal_dependencies() {
    let core = std::fs::read_to_string(repo_file("crates/core/Cargo.toml")).unwrap();
    for banned in ["ratatui", "crossterm", "termion", "tui"] {
        assert!(!core.contains(banned), "core must not depend on {banned}");
    }
    let cli = std::fs::read_to_string(repo_file("crates/cli/Cargo.toml")).unwrap();
    assert!(!cli.contains("ratatui") && !cli.contains("crossterm"));
    let watch = std::fs::read_to_string(repo_file("crates/watch/Cargo.toml")).unwrap();
    assert!(watch.contains("ratatui"));
}

// ---------------------------------------------------------------------
// v1.2: isolation
// ---------------------------------------------------------------------

fn survey_json(dir: &Path) -> Value {
    ok_json(dir, &["isolation"])["data"].clone()
}

fn backend_works(survey: &Value, name: &str) -> bool {
    survey["backends"]
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b["backend"] == name && b["works"] == true)
}

#[test]
fn init_writes_the_sandbox_table_with_safe_defaults() {
    let dir = project();
    let text = std::fs::read_to_string(dir.path().join(".rules-evolve/config.toml")).unwrap();
    assert!(text.contains("[sandbox]"));
    assert!(text.contains("isolation = \"auto\""));
    assert!(text.contains("image = \"docker.io/library/alpine:3.20\""));
    assert!(text.contains("max_processes = 256"));
}

#[test]
fn isolation_command_reports_what_works_and_what_auto_picks() {
    let dir = project();
    let d = survey_json(dir.path());
    assert_eq!(d["configured"], "auto");
    let names: Vec<&str> = d["backends"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["backend"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["bwrap", "podman", "docker"]);
    // Every backend that does not work says why.
    for b in d["backends"].as_array().unwrap() {
        if b["works"] == false {
            assert!(!b["problem"].as_str().unwrap().is_empty());
        }
    }
    // auto is the first one that works, or none.
    let first = ["bwrap", "podman", "docker"]
        .into_iter()
        .find(|n| backend_works(&d, n))
        .unwrap_or("none");
    assert_eq!(d["auto"], first);
    let out = run(dir.path(), &["isolation"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("auto would use:"));
}

#[test]
fn run_labels_none_as_unsafe_and_records_it_in_the_log() {
    let dir = demo_project();
    let out = run(
        dir.path(),
        &["run", "--repeats", "1", "--isolation", "none"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("Isolation: none (UNSAFE"), "{text}");

    let v = ok_json(
        dir.path(),
        &["run", "--repeats", "2", "--isolation", "none"],
    );
    assert_eq!(v["data"]["isolation"], "none");
    let runs = ok_json(dir.path(), &["log", "--kind", "run_recorded"]);
    for e in runs["data"]["events"].as_array().unwrap() {
        assert_eq!(e["payload"]["isolation"], "none");
    }
}

#[test]
fn run_uses_the_backend_auto_picks_and_records_it() {
    let dir = demo_project();
    let want = survey_json(dir.path())["auto"]
        .as_str()
        .unwrap()
        .to_string();
    let v = ok_json(dir.path(), &["run", "--repeats", "1"]);
    assert_eq!(v["data"]["isolation"], want.as_str());
    let runs = ok_json(dir.path(), &["log", "--kind", "run_recorded"]);
    let events = runs["data"]["events"].as_array().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["payload"]["isolation"], want.as_str());
    // The demo tasks pass the same way under isolation or without it.
    assert!(v["data"]["runs"][0]["passed"].as_u64().is_some());
}

#[test]
fn the_isolation_setting_in_config_and_the_flag_both_work() {
    let dir = demo_project();
    let cfg = dir.path().join(".rules-evolve/config.toml");
    let text = std::fs::read_to_string(&cfg).unwrap();
    std::fs::write(
        &cfg,
        text.replace("isolation = \"auto\"", "isolation = \"none\""),
    )
    .unwrap();
    let v = ok_json(dir.path(), &["run", "--repeats", "1"]);
    assert_eq!(v["data"]["isolation"], "none");
    // The flag wins over the config.
    let want = survey_json(dir.path());
    if backend_works(&want, "bwrap") {
        let v = ok_json(
            dir.path(),
            &["run", "--repeats", "2", "--isolation", "bwrap"],
        );
        assert_eq!(v["data"]["isolation"], "bwrap");
    }
}

#[test]
fn unknown_or_missing_backends_are_clear_errors_not_silent_fallbacks() {
    let dir = demo_project();
    let out = run(dir.path(), &["run", "--isolation", "chroot", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(json_of(&out)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("unknown isolation"));
    let d = survey_json(dir.path());
    for name in ["docker", "podman"] {
        if !backend_works(&d, name) {
            let out = run(dir.path(), &["run", "--isolation", name, "--json"]);
            assert_eq!(out.status.code(), Some(1));
            let msg = json_of(&out)["error"]["message"]
                .as_str()
                .unwrap()
                .to_string();
            assert!(msg.contains("does not work here"), "{msg}");
        }
    }
    // Nothing was run or recorded by any of these.
    let runs = ok_json(dir.path(), &["log", "--kind", "run_recorded"]);
    assert_eq!(runs["data"]["total"], 0);
}

#[test]
fn the_demo_replay_still_gives_the_same_lineage_under_isolation() {
    // evolved_demo runs the committed replay with the default (auto) setting.
    let dir = evolved_demo();
    let v = ok_json(dir.path(), &["status"]);
    assert_eq!(v["data"]["valid"], true);
    let runs = ok_json(dir.path(), &["log", "--kind", "run_recorded"]);
    let want = survey_json(dir.path())["auto"]
        .as_str()
        .unwrap()
        .to_string();
    for e in runs["data"]["events"].as_array().unwrap() {
        assert_eq!(e["payload"]["isolation"], want.as_str());
    }
}

// ---------------------------------------------------------------------
// v1.2.2: shadow warnings
// ---------------------------------------------------------------------

/// The demo project plus a hidden spec that sits beside a visible readme.
fn risky_demo() -> tempfile::TempDir {
    let dir = demo_project();
    let root = dir.path();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/spec.txt"), "spec\n").unwrap();
    std::fs::write(root.join("docs/readme.txt"), "readme\n").unwrap();
    let tasks = root.join(".rules-evolve/tasks.toml");
    let t = std::fs::read_to_string(&tasks).unwrap();
    std::fs::write(
        &tasks,
        t.replace(
            "hidden = [\"check.sh\"]",
            "hidden = [\"check.sh\", \"docs/spec.txt\"]",
        ),
    )
    .unwrap();
    dir
}

fn set_shadow_mode(dir: &Path, mode: &str) {
    let cfg = dir.join(".rules-evolve/config.toml");
    let t = std::fs::read_to_string(&cfg).unwrap();
    assert!(t.contains("shadow_warnings = \"warn\""));
    std::fs::write(
        &cfg,
        t.replace(
            "shadow_warnings = \"warn\"",
            &format!("shadow_warnings = \"{mode}\""),
        ),
    )
    .unwrap();
}

#[test]
fn the_demo_has_no_shadow_warnings_anywhere() {
    let dir = demo_project();
    let run_json = ok_json(dir.path(), &["run", "--repeats", "1"]);
    assert_eq!(run_json["data"]["warnings"], serde_json::json!([]));
    let iso = ok_json(dir.path(), &["isolation"]);
    assert_eq!(iso["data"]["warnings"], serde_json::json!([]));
    assert_eq!(iso["data"]["shadow_warnings"], "warn");
    let text = String::from_utf8(run(dir.path(), &["run", "--repeats", "1"]).stdout).unwrap();
    assert!(!text.contains("warning:"), "{text}");
    // evolve on the demo replay: no warnings in the report either.
    let replay = demo_replay_path();
    let fresh = demo_project();
    let v = ok_json(
        fresh.path(),
        &[
            "evolve",
            "--generations",
            "1",
            "--seed",
            "7",
            "--provider",
            "replay",
            "--replay-file",
            replay.to_str().unwrap(),
        ],
    );
    assert_eq!(v["data"]["warnings"], serde_json::json!([]));
}

#[test]
fn init_reports_an_empty_warnings_array() {
    let dir = project();
    let out = run(dir.path(), &["init", "--model", "m", "--json"]);
    // `project()` already initialized this folder, so use a fresh one.
    assert!(!out.status.success());
    let fresh = tempfile::tempdir().unwrap();
    std::fs::write(fresh.path().join("AGENTS.md"), "rules\n").unwrap();
    let v = ok_json(fresh.path(), &["init", "--model", "m"]);
    assert_eq!(v["data"]["warnings"], serde_json::json!([]));
}

#[test]
fn a_risky_layout_is_shown_as_text_and_json_and_does_not_block() {
    let dir = risky_demo();
    let out = run(dir.path(), &["run", "--repeats", "1"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("warning: docs/ holds the hidden file(s) docs/spec.txt next to visible files (docs/readme.txt)"), "{text}");
    assert!(
        text.contains("  fix: Add \"docs\" to hidden in tasks.toml"),
        "{text}"
    );
    // The warning comes before the results.
    assert!(text.find("warning:").unwrap() < text.find("run 0:").unwrap());

    let v = ok_json(dir.path(), &["run", "--repeats", "2"]);
    let w = &v["data"]["warnings"];
    assert_eq!(w.as_array().unwrap().len(), 1);
    assert_eq!(w[0]["kind"], "hidden_file_beside_visible_files");
    assert_eq!(w[0]["paths"], serde_json::json!(["docs", "docs/spec.txt"]));
    assert!(w[0]["message"].as_str().unwrap().contains("docs/"));
    assert!(w[0]["fix"].as_str().unwrap().contains("hidden"));
}

#[test]
fn evolve_and_isolation_show_the_warnings_too() {
    let dir = risky_demo();
    let iso = ok_json(dir.path(), &["isolation"]);
    assert_eq!(iso["data"]["warnings"].as_array().unwrap().len(), 1);
    let text = String::from_utf8(run(dir.path(), &["isolation"]).stdout).unwrap();
    assert!(text.contains("Shadow warnings: warn"));
    assert!(text.contains("warning: docs/"));

    // evolve with the mock model: the warning leads the report.
    let out = run(dir.path(), &["evolve", "--generations", "1", "--seed", "1"]);
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        text.starts_with("warning: docs/"),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let other = risky_demo();
    let v = ok_json(
        other.path(),
        &["evolve", "--generations", "1", "--seed", "1"],
    );
    assert_eq!(v["data"]["warnings"].as_array().unwrap().len(), 1);
}

#[test]
fn error_mode_refuses_to_run_and_records_nothing() {
    let dir = risky_demo();
    set_shadow_mode(dir.path(), "error");
    for args in [
        vec!["run", "--repeats", "1", "--json"],
        vec!["evolve", "--generations", "1", "--json"],
    ] {
        let out = run(dir.path(), &args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        let msg = json_of(&out)["error"]["message"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(msg.contains("shadow_warnings = \"error\""), "{msg}");
        assert!(msg.contains("docs/spec.txt"));
    }
    let log = ok_json(dir.path(), &["log"]);
    assert_eq!(log["data"]["total"], 1);
    // `isolation` still reports instead of failing.
    let iso = ok_json(dir.path(), &["isolation"]);
    assert_eq!(iso["data"]["shadow_warnings"], "error");
    assert_eq!(iso["data"]["warnings"].as_array().unwrap().len(), 1);
}

#[test]
fn off_mode_hides_the_warnings() {
    let dir = risky_demo();
    set_shadow_mode(dir.path(), "off");
    let v = ok_json(dir.path(), &["run", "--repeats", "1"]);
    assert_eq!(v["data"]["warnings"], serde_json::json!([]));
    let iso = ok_json(dir.path(), &["isolation"]);
    assert_eq!(iso["data"]["warnings"], serde_json::json!([]));
}

#[test]
fn hiding_the_whole_folder_clears_the_warning() {
    let dir = risky_demo();
    let tasks = dir.path().join(".rules-evolve/tasks.toml");
    let t = std::fs::read_to_string(&tasks).unwrap();
    std::fs::write(&tasks, t.replace("\"docs/spec.txt\"", "\"docs\"")).unwrap();
    let v = ok_json(dir.path(), &["run", "--repeats", "1"]);
    assert_eq!(v["data"]["warnings"], serde_json::json!([]));
}

#[test]
fn local_output_files_must_not_land_where_the_model_reads() {
    let dir = project();
    let d = dir.path();
    // nothing listens on port 9: the guard must fire before any connection
    let base = [
        "run",
        "--provider",
        "local",
        "--local-url",
        "http://127.0.0.1:9",
    ];
    for (flag, path) in [
        ("--record", "rec.jsonl"),
        ("--local-transcript", "sub/../t.jsonl"),
    ] {
        let mut args: Vec<&str> = base.to_vec();
        args.extend([flag, path, "--json"]);
        let out = run(d, &args);
        assert_eq!(out.status.code(), Some(1));
        let msg = json_of(&out)["error"]["message"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(msg.contains("inside the repo"), "{msg}");
    }
    std::fs::create_dir(d.join("sub")).unwrap();
    let out = run(
        d,
        &[&base[..], &["--record", "sub/rec.jsonl", "--json"]].concat(),
    );
    assert!(json_of(&out)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("inside the repo"));
    // under .rules-evolve or outside the repo is fine: the run then fails only
    // because no server is listening
    let outside = tempfile::tempdir().unwrap();
    let rec = outside.path().join("rec.jsonl");
    for path in [".rules-evolve/rec.jsonl", rec.to_str().unwrap()] {
        let out = run(d, &[&base[..], &["--record", path, "--json"]].concat());
        assert_eq!(out.status.code(), Some(1));
        let msg = json_of(&out)["error"]["message"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(!msg.contains("inside the repo"), "{msg}");
    }
}

/// Five train tasks and three held out ones. NARROW helps train tasks only.
fn held_out_project() -> tempfile::TempDir {
    let dir = bench_project(5);
    let root = dir.path();
    let mut tasks = std::fs::read_to_string(root.join(".rules-evolve/tasks.toml")).unwrap();
    let mut mock = std::fs::read_to_string(root.join(".rules-evolve/mock.toml")).unwrap();
    let train: Vec<String> = (0..N_TASKS).map(|i| format!("\"t{i}\"")).collect();
    mock.push_str(&format!(
        "[[marker]]\ntext = \"NARROW\"\ndelta = 0.6\ntasks = [{}]\n",
        train.join(", ")
    ));
    for i in 0..3 {
        tasks.push_str(&format!(
            "[[task]]\nid = \"h{i}\"\nprompt = \"held out {i}\"\ncheck = \"sh check.sh h{i}\"\nsplit = \"held_out\"\n"
        ));
        mock.push_str(&format!(
            "[[answer]]\ntask = \"h{i}\"\npath = \"answers/h{i}.txt\"\npass_content = \"ok\\n\"\nfail_content = \"no\\n\"\n"
        ));
    }
    std::fs::write(root.join(".rules-evolve/tasks.toml"), tasks).unwrap();
    std::fs::write(root.join(".rules-evolve/mock.toml"), mock).unwrap();
    dir
}

#[test]
fn held_out_gate_through_the_cli_and_export() {
    let dir = held_out_project();
    let d = dir.path();
    let narrow = evolve(d, "base rules\nNARROW\n", "add the narrow hint");
    assert_eq!(narrow["decision"]["verdict"], "overfit", "{narrow}");
    assert!(narrow["decision"]["gain_pp"].as_f64().unwrap() >= 10.0);
    assert_eq!(narrow["held_out_min_paired_wins"], 3);
    assert_eq!(narrow["decision"]["held_out"]["passed"], false);
    let general = evolve(d, "base rules\nGOOD\n", "add the good hint");
    assert_eq!(general["decision"]["verdict"], "accepted", "{general}");
    assert_eq!(general["decision"]["held_out"]["passed"], true);

    // human text names the gate
    let file = d.join("c2.txt");
    std::fs::write(&file, "base rules\nNARROW\nx\n").unwrap();
    let p = ok_json(
        d,
        &[
            "propose",
            "--file",
            file.to_str().unwrap(),
            "--hypothesis",
            "h",
            "--parent",
            "root",
        ],
    );
    let child = p["data"]["child"].as_str().unwrap().to_string();
    ok_json(d, &["run", "--version", &child]);
    let out = run(d, &["verify", "--child", &child]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("OVERFIT"), "{text}");
    assert!(text.contains("held out: parent median"), "{text}");
    let st = String::from_utf8_lossy(&run(d, &["status"]).stdout).to_string();
    assert!(st.contains("overfit 2"), "{st}");

    // the export carries the held out fields and validates
    let out = d.join("tree.json");
    ok_json(d, &["export", "--out", out.to_str().unwrap()]);
    let doc = read_json(&out);
    assert_valid(&schema_validator(), &doc);
    assert_eq!(doc["summary"]["overfit"], 2);
    assert_eq!(doc["config"]["held_out_min_paired_wins"], 3);
    let edges = doc["edges"].as_array().unwrap();
    assert_eq!(edges[0]["verdict"], "overfit");
    assert!(edges[0]["held_out_gain_pp"].is_number());
    let run0 = &doc["nodes"][0]["runs"][0];
    assert_eq!(run0["total"], N_TASKS as u64);
    assert_eq!(run0["held_out_total"], 3);
    assert!(doc["nodes"][0]["held_out_median_pass_rate"].is_number());
    assert!(doc["series"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["outcome"] == "overfit"));

    // the digest has an overfit section
    let dg = String::from_utf8_lossy(&run(d, &["digest"]).stdout).to_string();
    assert!(dg.contains("### Overfit"), "{dg}");
}

#[test]
fn apply_is_a_dry_run_until_yes_and_needs_force_for_non_accepted() {
    let dir = evolved_demo();
    let d = dir.path();
    let before = std::fs::read_to_string(d.join("AGENTS.md")).unwrap();
    let v = ok_json(d, &["apply"]);
    assert_eq!(v["command"], "apply");
    assert_eq!(v["data"]["outcome"], "dry_run");
    assert_eq!(v["data"]["status"], "accepted");
    assert!(v["data"]["diff"]
        .as_str()
        .unwrap()
        .starts_with("diff --git a/AGENTS.md"));
    assert_eq!(
        std::fs::read_to_string(d.join("AGENTS.md")).unwrap(),
        before
    );

    let text = String::from_utf8_lossy(&run(d, &["apply"]).stdout).to_string();
    assert!(text.contains("Dry run: nothing was written"), "{text}");

    let v = ok_json(d, &["apply", "--yes"]);
    assert_eq!(v["data"]["outcome"], "written");
    let status = ok_json(d, &["status"]);
    let current = status["data"]["tree"]["current"]
        .as_str()
        .unwrap()
        .to_string();
    let blob = std::fs::read(d.join(".rules-evolve/blobs").join(&current)).unwrap();
    assert_eq!(std::fs::read(d.join("AGENTS.md")).unwrap(), blob);
    assert_eq!(
        status["data"]["tree"]["applied"][0]["version"],
        current.as_str()
    );
    let log = ok_json(d, &["log", "--kind", "applied"]);
    assert_eq!(log["data"]["events"].as_array().unwrap().len(), 1);
    assert_eq!(ok_json(d, &["verify-log"])["data"]["valid"], true);
    // the export still validates with an applied event in the log
    let out = d.join("t.json");
    ok_json(d, &["export", "--out", out.to_str().unwrap()]);
    assert_valid(&schema_validator(), &read_json(&out));

    // a rejected version is refused without --force, and nothing changes
    let rejected = status["data"]["tree"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["verdict"] == "rejected")
        .unwrap()["child"]
        .as_str()
        .unwrap()
        .to_string();
    let out = run(d, &["apply", "--version", &rejected, "--yes"]);
    assert_ne!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--force"));
    assert_eq!(std::fs::read(d.join("AGENTS.md")).unwrap(), blob);
    let v = ok_json(
        d,
        &["apply", "--version", &rejected[..8], "--yes", "--force"],
    );
    assert_eq!(v["data"]["forced"], true);
    assert_eq!(v["data"]["status"], "rejected");
}

/// A fake OpenAI compatible server on 127.0.0.1 that answers every task with
/// a passing file. Returns the port and the raw requests it saw.
fn fake_paid_server() -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut s) = conn else { return };
            let mut raw = Vec::new();
            let mut buf = [0u8; 16384];
            loop {
                let n = s.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
                let t = String::from_utf8_lossy(&raw).to_string();
                if let Some(end) = t.find("\r\n\r\n") {
                    let len: usize = t[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if raw.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            let t = String::from_utf8_lossy(&raw).to_string();
            let task = t
                .split("solve task ")
                .nth(1)
                .map(|r| {
                    r.chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect::<String>()
                })
                .unwrap_or_default();
            log.lock().unwrap().push(t);
            let content = format!("FILE: answers/t{task}.txt\n```\nok\n```");
            let body = serde_json::json!({
                "choices": [{"message": {"content": content}}],
                "usage": {"total_tokens": 50}
            })
            .to_string();
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(reply.as_bytes());
        }
    });
    (port, seen)
}

#[test]
fn paid_providers_need_allow_spend_a_model_a_key_and_isolation() {
    const KEY: &str = "sk-cli-test-NOT-A-REAL-KEY-42";
    let dir = bench_project(5);
    let d = dir.path();
    let (port, seen) = fake_paid_server();
    let url = format!("http://127.0.0.1:{port}/v1");
    let paid = |extra: &[&str], key: Option<&str>| {
        let mut args = vec![
            "run",
            "--repeats",
            "1",
            "--provider",
            "openai",
            "--api-url",
            &url,
        ];
        args.extend_from_slice(extra);
        args.push("--json");
        let mut c = bin();
        c.current_dir(d).args(&args).env_remove("OPENAI_API_KEY");
        if let Some(k) = key {
            c.env("OPENAI_API_KEY", k);
        }
        c.output().unwrap()
    };
    let msg = |o: &Output| {
        json_of(o)["error"]["message"]
            .as_str()
            .unwrap_or("")
            .to_string()
    };
    let events_before = ok_json(d, &["log"])["data"]["events"]
        .as_array()
        .unwrap()
        .len();

    let o = paid(&["--api-model", "m1"], Some(KEY));
    assert_eq!(o.status.code(), Some(1));
    assert!(msg(&o).contains("--allow-spend"), "{}", msg(&o));
    let o = paid(&["--allow-spend"], Some(KEY));
    assert!(msg(&o).contains("--api-model"), "{}", msg(&o));
    let o = paid(&["--allow-spend", "--api-model", "m1"], None);
    assert!(msg(&o).contains("OPENAI_API_KEY is not set"), "{}", msg(&o));
    let o = paid(
        &["--allow-spend", "--api-model", "m1", "--isolation", "none"],
        Some(KEY),
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(msg(&o).to_lowercase().contains("isolat"), "{}", msg(&o));
    // the free providers refuse the paid flags
    let o = run(d, &["run", "--allow-spend", "--json"]);
    assert!(msg(&o).contains("only apply to --provider anthropic or openai"));
    assert!(
        seen.lock().unwrap().is_empty(),
        "nothing may be sent before all checks pass"
    );
    let events_after = ok_json(d, &["log"])["data"]["events"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(events_before, events_after);

    // with everything in place and bwrap working, a run goes through the fake
    // server, and the key is nowhere on disk
    let want = survey_json(d);
    if !backend_works(&want, "bwrap") {
        return;
    }
    let rec = tempfile::tempdir().unwrap();
    let rec_file = rec.path().join("rec.jsonl");
    let o = paid(
        &[
            "--allow-spend",
            "--api-model",
            "m1",
            "--isolation",
            "bwrap",
            "--record",
            rec_file.to_str().unwrap(),
        ],
        Some(KEY),
    );
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    let v = json_of(&o);
    assert_eq!(v["data"]["provider"], "openai");
    assert_eq!(v["data"]["runs"][0]["passed"], N_TASKS as u64);
    assert_eq!(seen.lock().unwrap().len(), N_TASKS);
    assert!(seen.lock().unwrap()[0].contains(KEY));
    let mut on_disk = std::fs::read_to_string(&rec_file).unwrap();
    for entry in walk_files(&d.join(".rules-evolve")) {
        on_disk.push_str(&String::from_utf8_lossy(&std::fs::read(entry).unwrap()));
    }
    assert!(!on_disk.contains(KEY));
    assert!(!String::from_utf8_lossy(&o.stdout).contains(KEY));
    assert!(!String::from_utf8_lossy(&o.stderr).contains(KEY));
}

fn walk_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk_files(&p));
        } else {
            out.push(p);
        }
    }
    out
}
