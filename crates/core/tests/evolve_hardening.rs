//! Tests for the problems the real 7B runs hit (v2.7.2), plus positive
//! controls that show the loop can say yes.
//!
//! Every test here is deterministic and free: the mock model, a scripted
//! model on top of it, the replay provider, or a fake local HTTP server.

use bogovolution_core::bench::{propose, run_version, verify_child, RunOptions};
use bogovolution_core::event::{EventKind, Payload, TaskStatus, VerdictKind};
use bogovolution_core::init::{init, InitOptions};
use bogovolution_core::local::{LocalModel, LocalSettings};
use bogovolution_core::model::{
    MockConfig, MockModel, Model, ModelRequest, ModelResponse, MutationRequest, MutationResponse,
    ProviderKind,
};
use bogovolution_core::mutate::{evolve, EvolveOptions, EvolveReport};
use bogovolution_core::replay::{RecordingModel, ReplayModel};
use bogovolution_core::repo::Repo;
use bogovolution_core::tree::Tree;
use bogovolution_core::Error;
use std::cell::{Cell, RefCell};
use std::io::{Read, Write};
use std::net::TcpListener;
use tempfile::{tempdir, TempDir};

const BASE: &str = "base rules\n";

/// Mock settings: base pass rate 0.3. GENERAL lifts every task, NARROW only
/// the train tasks t0 to t4. Held out tasks are h0 to h2 when asked for.
fn mock_toml(held_out: bool) -> String {
    let mut m = String::from(
        "seed = 2\nbase_pass_probability = 0.3\ntokens_per_call = 100\n\
         [[marker]]\ntext = \"GENERAL\"\ndelta = 0.6\n\
         [[marker]]\ntext = \"NARROW\"\ndelta = 0.6\ntasks = [\"t0\", \"t1\", \"t2\", \"t3\", \"t4\"]\n",
    );
    for (id, _) in ids(held_out) {
        m.push_str(&format!(
            "[[answer]]\ntask = \"{id}\"\npath = \"answers/{id}.txt\"\npass_content = \"ok\\n\"\nfail_content = \"no\\n\"\n"
        ));
    }
    m
}

fn ids(held_out: bool) -> Vec<(String, bool)> {
    let mut v: Vec<(String, bool)> = (0..5).map(|i| (format!("t{i}"), false)).collect();
    if held_out {
        v.extend((0..3).map(|i| (format!("h{i}"), true)));
    }
    v
}

fn project(held_out: bool) -> (TempDir, Repo) {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("AGENTS.md"), BASE).unwrap();
    std::fs::write(root.join("check.sh"), "grep -qx ok \"answers/$1.txt\"\n").unwrap();
    init(&InitOptions {
        dir: root.to_path_buf(),
        rules_file: None,
        model: "mock-model".into(),
    })
    .unwrap();
    let mut tasks = String::from("[sandbox]\nhidden = [\"check.sh\"]\n");
    for (id, held) in ids(held_out) {
        tasks.push_str(&format!(
            "[[task]]\nid = \"{id}\"\nprompt = \"p {id}\"\ncheck = \"sh check.sh {id}\"\n{}",
            if held { "split = \"held_out\"\n" } else { "" }
        ));
    }
    std::fs::write(root.join(".bogo/tasks.toml"), tasks).unwrap();
    std::fs::write(root.join(".bogo/mock.toml"), mock_toml(held_out)).unwrap();
    let repo = Repo::open(root).unwrap();
    (dir, repo)
}

fn set_config(repo: &Repo, from: &str, to: &str) {
    let p = repo.store().root().join("config.toml");
    let t = std::fs::read_to_string(&p).unwrap();
    assert!(t.contains(from), "config has no {from:?}");
    std::fs::write(&p, t.replacen(from, to, 1)).unwrap();
}

fn mock(held_out: bool) -> MockModel {
    MockModel::new(toml::from_str::<MockConfig>(&mock_toml(held_out)).unwrap())
}

fn kinds(repo: &Repo, kind: EventKind) -> usize {
    repo.events_of(Some(kind)).unwrap().len()
}

fn verdicts(repo: &Repo) -> Vec<VerdictKind> {
    repo.events_of(Some(EventKind::Verdict))
        .unwrap()
        .iter()
        .map(|e| match Payload::from_event(e).unwrap() {
            Payload::Verdict(v) => v.verdict,
            _ => unreachable!(),
        })
        .collect()
}

fn log_ok(repo: &Repo) {
    let v = repo.verify().unwrap();
    assert!(v.chain.valid, "{:?}", v.chain.problems);
}

/// The mock for task answers, with a fixed list of mutation answers. Each
/// mutation call takes the next one; the seed of every call is kept.
struct Scripted {
    inner: MockModel,
    script: RefCell<Vec<Result<(String, String), String>>>,
    seeds: RefCell<Vec<u64>>,
    prompts: RefCell<Vec<String>>,
}

impl Scripted {
    /// `Ok((rules, hypothesis))` is an answer, `Err(why)` an unreadable one.
    fn new(held_out: bool, script: Vec<Result<(&str, &str), &str>>) -> Scripted {
        Scripted {
            inner: mock(held_out),
            script: RefCell::new(
                script
                    .into_iter()
                    .rev()
                    .map(|s| {
                        s.map(|(r, h)| (r.to_string(), h.to_string()))
                            .map_err(|e| e.to_string())
                    })
                    .collect(),
            ),
            seeds: RefCell::new(Vec::new()),
            prompts: RefCell::new(Vec::new()),
        }
    }
}

impl Model for Scripted {
    fn name(&self) -> String {
        "mock".into()
    }
    fn kind(&self) -> ProviderKind {
        ProviderKind::Local
    }
    fn estimate_tokens(&self, r: &ModelRequest) -> u64 {
        self.inner.estimate_tokens(r)
    }
    fn complete(&self, r: &ModelRequest) -> bogovolution_core::Result<ModelResponse> {
        self.inner.complete(r)
    }
    fn estimate_mutation_tokens(&self, _r: &MutationRequest) -> u64 {
        10
    }
    fn propose_mutation(&self, r: &MutationRequest) -> bogovolution_core::Result<MutationResponse> {
        self.seeds.borrow_mut().push(r.seed);
        self.prompts.borrow_mut().push(r.prompt.to_string());
        match self.script.borrow_mut().pop().expect("script ran out") {
            Ok((new_rules, hypothesis)) => Ok(MutationResponse {
                new_rules,
                hypothesis,
                tokens_used: 10,
            }),
            Err(why) => Err(Error::BadReply(why)),
        }
    }
}

fn evolve_n(repo: &Repo, model: &dyn Model, n: u32) -> bogovolution_core::Result<EvolveReport> {
    evolve(
        repo,
        model,
        &EvolveOptions {
            generations: n,
            base_seed: 7,
            total: false,
        },
    )
}

// ---------------------------------------------------------------- (a) repeats

#[test]
fn a_repeated_mutation_is_retried_with_a_new_seed_until_it_is_new() {
    let (_d, repo) = project(false);
    set_config(&repo, "retries_on_refusal = 0", "retries_on_refusal = 2");
    let g1 = "base rules\nGENERAL\n";
    let m = Scripted::new(
        false,
        vec![
            Ok((g1, "general hint")),
            // generation 2: the exact text of generation 1, then the same
            // text with different whitespace, then something new
            Ok((g1, "general hint again")),
            Ok(("base rules\n\n  GENERAL  \n", "general hint, reformatted")),
            Ok((
                "base rules\nGENERAL\nkeep functions short\n",
                "short functions",
            )),
        ],
    );
    let report = evolve_n(&repo, &m, 2).unwrap();
    let g = &report.generations[1];
    assert_eq!(g.refused_attempts.len(), 2, "{:?}", g.refused_attempts);
    // generation 1 was accepted, so its text is now the parent
    assert!(
        g.refused_attempts[0].contains("identical to the parent"),
        "{:?}",
        g.refused_attempts
    );
    assert!(g.refused_attempts[1].contains("only in whitespace"));
    assert!(g.mutation.result.child.is_some());
    assert!(g.verdict.is_some());
    // every attempt asked with its own seed, and every one is in the log
    let seeds = m.seeds.borrow().clone();
    let mut unique = seeds.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), seeds.len(), "{seeds:?}");
    assert_eq!(kinds(&repo, EventKind::MutationRefused), 2);
    assert_eq!(report.refused, 2);
    log_ok(&repo);
}

#[test]
fn without_retries_a_repeat_ends_the_generation_as_before() {
    let (_d, repo) = project(false);
    let g1 = "base rules\nGENERAL\n";
    let m = Scripted::new(false, vec![Ok((g1, "general")), Ok((g1, "general again"))]);
    let report = evolve_n(&repo, &m, 2).unwrap();
    assert!(report.generations[1].mutation.result.child.is_none());
    assert!(report.generations[1].refused_attempts.is_empty());
    assert_eq!(report.refused, 1);
    assert_eq!(m.seeds.borrow().len(), 2);
}

#[test]
fn whitespace_only_changes_are_refused() {
    let (_d, repo) = project(false);
    let cfg = repo.config().unwrap();
    let out = propose(&repo, &cfg, "root", b"base   rules\n\n", "reflow").unwrap();
    assert_eq!(out.outcome, "refused");
    assert!(out.reason.unwrap().contains("the parent"));
    let a = propose(&repo, &cfg, "root", b"base rules\nA\n", "add A").unwrap();
    assert_eq!(a.outcome, "proposed");
    let again = propose(&repo, &cfg, "root", b"base rules\n A \n", "add A again").unwrap();
    assert_eq!(again.outcome, "refused");
    assert!(again.reason.unwrap().contains("already in the tree"));
}

#[test]
fn unreadable_empty_and_oversized_answers_are_refused_and_retried() {
    let (_d, repo) = project(false);
    set_config(&repo, "retries_on_refusal = 0", "retries_on_refusal = 3");
    let big = format!("base rules\n{}\n", "x".repeat(5000));
    let m = Scripted::new(
        false,
        vec![
            Err("no rules block in the reply"),
            Ok(("   \n", "delete everything")),
            Ok((big.as_str(), "rewrite everything")),
            Ok(("base rules\nGENERAL\n", "general")),
        ],
    );
    let report = evolve_n(&repo, &m, 1).unwrap();
    let g = &report.generations[0];
    assert_eq!(g.refused_attempts.len(), 3, "{:?}", g.refused_attempts);
    assert!(g.refused_attempts[0].contains("no rules block"));
    assert!(g.refused_attempts[1].contains("empty"));
    assert!(g.refused_attempts[2].contains("over the limit"));
    assert!(g.mutation.result.child.is_some());
    log_ok(&repo);
}

#[test]
fn token_cap_refusals_are_not_retried() {
    let (_d, repo) = project(false);
    set_config(&repo, "retries_on_refusal = 0", "retries_on_refusal = 3");
    /// Claims more tokens for a mutation than one run may use.
    struct Greedy(MockModel);
    impl Model for Greedy {
        fn name(&self) -> String {
            "mock".into()
        }
        fn kind(&self) -> ProviderKind {
            ProviderKind::Local
        }
        fn estimate_tokens(&self, r: &ModelRequest) -> u64 {
            self.0.estimate_tokens(r)
        }
        fn complete(&self, r: &ModelRequest) -> bogovolution_core::Result<ModelResponse> {
            self.0.complete(r)
        }
        fn estimate_mutation_tokens(&self, _r: &MutationRequest) -> u64 {
            10_000_000
        }
    }
    let report = evolve_n(&repo, &Greedy(mock(false)), 1).unwrap();
    assert!(report.generations[0].refused_attempts.is_empty());
    assert_eq!(kinds(&repo, EventKind::MutationRefused), 1);
}

#[test]
fn a_replay_recorded_before_retries_still_replays_with_retries_on() {
    // Record two generations, the second refused, with no retries.
    let (_d, repo) = project(false);
    let rec = tempdir().unwrap();
    let file = rec.path().join("r.jsonl");
    let g1 = "base rules\nGENERAL\n";
    let recorder = RecordingModel::new(
        Box::new(Scripted::new(
            false,
            vec![Ok((g1, "general")), Ok((g1, "again"))],
        )),
        &file,
    )
    .unwrap();
    let first = evolve_n(&repo, &recorder, 2).unwrap();
    drop(recorder);
    // Replay it where retries are on: the retry has no recorded answer, so
    // the refusal stands and the lineage is the same.
    let (_d2, again) = project(false);
    set_config(&again, "retries_on_refusal = 0", "retries_on_refusal = 2");
    let replayed = evolve_n(&again, &ReplayModel::load(&file).unwrap(), 2).unwrap();
    assert_eq!(first.end, replayed.end);
    assert_eq!(first.refused, replayed.refused);
    assert_eq!(verdicts(&repo), verdicts(&again));
    assert!(replayed.generations[1].mutation.result.child.is_none());
}

// ------------------------------------------------------- (b) server dies, (e) resume

/// The mock, until `calls_left` task calls are used up; then every call
/// fails the way a dead local server does.
struct DiesAfter {
    inner: Box<dyn Model>,
    calls_left: Cell<u32>,
}

impl Model for DiesAfter {
    fn name(&self) -> String {
        "mock".into()
    }
    fn kind(&self) -> ProviderKind {
        ProviderKind::Local
    }
    fn estimate_tokens(&self, r: &ModelRequest) -> u64 {
        self.inner.estimate_tokens(r)
    }
    fn complete(&self, r: &ModelRequest) -> bogovolution_core::Result<ModelResponse> {
        if self.calls_left.get() == 0 {
            return Err(Error::Input(
                "local model call failed: could not connect to 127.0.0.1:8080: Connection refused (os error 111)".into(),
            ));
        }
        self.calls_left.set(self.calls_left.get() - 1);
        self.inner.complete(r)
    }
    fn estimate_mutation_tokens(&self, r: &MutationRequest) -> u64 {
        self.inner.estimate_mutation_tokens(r)
    }
    fn propose_mutation(&self, r: &MutationRequest) -> bogovolution_core::Result<MutationResponse> {
        self.inner.propose_mutation(r)
    }
}

fn general_then_narrow() -> Scripted {
    Scripted::new(
        true,
        vec![
            Ok(("base rules\nGENERAL\n", "a general hint")),
            Ok(("base rules\nGENERAL\nNARROW\n", "a narrow hint")),
        ],
    )
}

fn runs_of(repo: &Repo) -> Vec<(String, u32, u32, Option<u32>)> {
    let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
    let mut v: Vec<_> = tree
        .nodes
        .iter()
        .flat_map(|n| {
            n.runs
                .iter()
                .map(|r| (n.hash.clone(), r.run_index, r.passed, r.held_out_passed))
        })
        .collect();
    v.sort();
    v
}

#[test]
fn a_dead_server_stops_the_run_records_nothing_and_evolve_resumes_to_the_same_result() {
    // Control: the same evolve with a healthy model all the way.
    let (_c, control) = project(true);
    let full = evolve_n(&control, &general_then_narrow(), 2).unwrap();

    // 8 tasks per run, 5 runs of the root = 40 calls. Die 13 calls into the
    // first child, in its second run.
    let (_d, repo) = project(true);
    let dying = DiesAfter {
        inner: Box::new(general_then_narrow()),
        calls_left: Cell::new(40 + 13),
    };
    let err = evolve_n(&repo, &dying, 2).unwrap_err();
    assert!(matches!(err, Error::ModelUnavailable(_)), "{err}");
    let msg = err.to_string();
    assert!(msg.contains("was not recorded"), "{msg}");
    assert!(msg.contains("Connection refused"), "{msg}");
    // Only whole runs are in the log: the root's 5 and the child's first.
    let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
    assert_eq!(tree.edges.len(), 1);
    assert!(
        tree.edges[0].verdict.is_none(),
        "the cut generation has no verdict"
    );
    let child = tree.edges[0].child.clone();
    assert_eq!(tree.node(&child).unwrap().runs.len(), 1);
    assert_eq!(kinds(&repo, EventKind::Verdict), 0);
    let no_errors = tree
        .nodes
        .iter()
        .flat_map(|n| n.runs.iter())
        .flat_map(|r| r.tasks.iter())
        .all(|t| t.status != TaskStatus::ModelError);
    assert!(no_errors, "a model error was recorded");
    log_ok(&repo);

    // The server is back. The same command finishes the open child first
    // (no new mutation), then does generation 2.
    let healthy = Scripted::new(
        true,
        vec![Ok(("base rules\nGENERAL\nNARROW\n", "a narrow hint"))],
    );
    let resumed = evolve_n(&repo, &healthy, 2).unwrap();
    assert!(resumed.generations[0].resumed);
    assert_eq!(
        resumed.generations[0].mutation.result.child.as_deref(),
        Some(child.as_str())
    );
    assert!(!resumed.generations[1].resumed);
    assert_eq!(
        healthy.seeds.borrow().len(),
        1,
        "only generation 2 asked for a mutation"
    );
    assert_eq!(verdicts(&repo), verdicts(&control));
    assert_eq!(runs_of(&repo), runs_of(&control));
    assert_eq!(resumed.end, full.end);
    log_ok(&repo);
}

fn three_hints() -> Vec<Result<(&'static str, &'static str), &'static str>> {
    vec![
        Ok(("base rules\nGENERAL\n", "a general hint")),
        Ok(("base rules\nGENERAL\nNARROW\n", "a narrow hint")),
        Ok(("base rules\nGENERAL\nmore words\n", "a wordy hint")),
    ]
}

fn evolve_total(repo: &Repo, model: &dyn Model, n: u32) -> bogovolution_core::Result<EvolveReport> {
    evolve(
        repo,
        model,
        &EvolveOptions {
            generations: n,
            base_seed: 7,
            total: true,
        },
    )
}

#[test]
fn with_total_a_resumed_evolve_stops_at_the_same_total_of_generations() {
    // The first paid Claude run was cut off in generation 2 of 3 and the same
    // command (3 generations) made 3 more: 4 in all. With `total` the rerun
    // finishes generation 2 and makes generation 3, and nothing more.
    let (_c, control) = project(true);
    let full = evolve_total(&control, &Scripted::new(true, three_hints()), 3).unwrap();
    assert_eq!(full.generations.len(), 3);
    assert_eq!(full.generations_already_done, 0);

    // root 5 runs of 8 tasks, child 1 the same, then 13 calls into child 2
    let (_d, repo) = project(true);
    let dying = DiesAfter {
        inner: Box::new(Scripted::new(true, three_hints())),
        calls_left: Cell::new(40 + 40 + 13),
    };
    let err = evolve_total(&repo, &dying, 3).unwrap_err();
    assert!(matches!(err, Error::ModelUnavailable(_)), "{err}");
    let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
    assert_eq!(tree.edges.len(), 2);
    assert!(tree.edges[1].verdict.is_none());

    let healthy = Scripted::new(true, three_hints()[2..].to_vec());
    let resumed = evolve_total(&repo, &healthy, 3).unwrap();
    assert_eq!(resumed.generations_already_done, 1);
    let numbers: Vec<u32> = resumed.generations.iter().map(|g| g.generation).collect();
    assert_eq!(numbers, vec![2, 3]);
    assert!(resumed.generations[0].resumed);
    assert_eq!(healthy.seeds.borrow().len(), 1, "one new mutation");
    assert_eq!(verdicts(&repo), verdicts(&control));
    assert_eq!(runs_of(&repo), runs_of(&control));
    assert_eq!(resumed.end, full.end);

    // Once more: the total is reached, so nothing is run or asked for (the
    // empty script would panic on a mutation call).
    let idle = Scripted::new(true, vec![]);
    let again = evolve_total(&repo, &idle, 3).unwrap();
    assert!(again.generations.is_empty());
    assert_eq!(again.generations_already_done, 3);
    assert_eq!(kinds(&repo, EventKind::Verdict), 3);
    log_ok(&repo);

    // Without `total` the same resume would go one past (the old behaviour).
    let (_e, old) = project(true);
    let dying = DiesAfter {
        inner: Box::new(Scripted::new(true, three_hints())),
        calls_left: Cell::new(40 + 40 + 13),
    };
    evolve_n(&old, &dying, 3).unwrap_err();
    let mut more = three_hints()[2..].to_vec();
    more.push(Ok(("base rules\nGENERAL\nfour\n", "a fourth hint")));
    let past = evolve_n(&old, &Scripted::new(true, more), 3).unwrap();
    assert_eq!(past.generations.len(), 3);
    assert_eq!(kinds(&old, EventKind::Verdict), 4);
}

#[test]
fn total_counts_refused_attempts_and_children_from_propose() {
    let (_d, repo) = project(true);
    let model = Scripted::new(true, vec![Ok((BASE, "no change"))]);
    // generation 1: an unchanged file is refused
    let r = evolve_total(&repo, &model, 1).unwrap();
    assert_eq!(r.refused, 1);
    // a child added by hand and judged counts too
    let cfg = repo.config().unwrap();
    let c = propose(&repo, &cfg, "root", b"base rules\nGENERAL\n", "by hand").unwrap();
    let child = c.child.clone().unwrap();
    let opts = RunOptions {
        version: child.clone(),
        repeats: None,
        base_seed: 7,
    };
    run_version(&repo, &model, &opts).unwrap();
    verify_child(&repo, &child).unwrap();
    let idle = Scripted::new(true, vec![]);
    let r = evolve_total(&repo, &idle, 2).unwrap();
    assert_eq!(r.generations_already_done, 2);
    assert!(r.generations.is_empty());
}

#[test]
fn one_shot_agent_tells_the_mutation_model_what_the_agent_cannot_do() {
    let (_d, repo) = project(true);
    let model = Scripted::new(true, three_hints());
    evolve_n(&repo, &model, 1).unwrap();
    let old = model.prompts.borrow()[0].clone();
    assert!(!old.contains("single reply"), "off by default: {old}");

    let (_e, repo) = project(true);
    set_config(&repo, "one_shot_agent = false", "one_shot_agent = true");
    let model = Scripted::new(true, three_hints());
    evolve_n(&repo, &model, 1).unwrap();
    let p = model.prompts.borrow()[0].clone();
    assert!(
        p.contains("answers in a single reply. It cannot run commands, tests or checks"),
        "{p}"
    );
    assert!(
        p.contains("It cannot read these hidden paths: check.sh."),
        "{p}"
    );
    assert!(
        p.contains("Do not propose rules that need any of this."),
        "{p}"
    );
    // Only the constraint list changes.
    let without: String = p
        .lines()
        .filter(|l| !l.contains("single reply"))
        .map(|l| format!("{l}\n"))
        .collect();
    assert_eq!(without, old);
}

#[test]
fn tolerated_model_errors_are_recorded_as_failures_like_before() {
    let (_d, repo) = project(false);
    set_config(
        &repo,
        "max_model_errors_per_run = 0",
        "max_model_errors_per_run = 100",
    );
    let dying = DiesAfter {
        inner: Box::new(mock(false)),
        calls_left: Cell::new(2),
    };
    let report = run_version(
        &repo,
        &dying,
        &RunOptions {
            version: "root".into(),
            repeats: Some(1),
            base_seed: 7,
        },
    )
    .unwrap();
    let errors = report.runs[0]
        .tasks
        .iter()
        .filter(|t| t.status == TaskStatus::ModelError)
        .count();
    assert_eq!(errors, 3);
}

/// A tiny OpenAI compatible server that answers `answers` chat calls, each
/// writing `ok` for its task, and then goes away (connections refused).
fn fake_local_server(answers: usize) -> (u16, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let t = std::thread::spawn(move || {
        for _ in 0..answers {
            let Ok((mut s, _)) = listener.accept() else {
                return;
            };
            let mut data = Vec::new();
            let mut buf = [0u8; 8192];
            let mut want = None;
            loop {
                let n = s.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                data.extend_from_slice(&buf[..n]);
                if want.is_none() {
                    if let Some(p) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&data[..p]).to_ascii_lowercase();
                        let len = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        want = Some(p + 4 + len);
                    }
                }
                if want.is_some_and(|w| data.len() >= w) {
                    break;
                }
            }
            let req = String::from_utf8_lossy(&data);
            let task = (0..5)
                .map(|i| format!("t{i}"))
                .find(|t| req.contains(&format!("p {t}")))
                .unwrap_or_else(|| "t0".into());
            let content = format!("FILE: answers/{task}.txt\n```\nok\n```\n");
            let body = serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": content}}],
                "usage": {"total_tokens": 50}
            })
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
        // the listener is dropped here: the server is gone
    });
    (port, t)
}

/// The real local provider, presented as a local test model so the test
/// needs no bwrap.
struct ViaHttp(LocalModel);
impl Model for ViaHttp {
    fn name(&self) -> String {
        "local".into()
    }
    fn kind(&self) -> ProviderKind {
        ProviderKind::Local
    }
    fn estimate_tokens(&self, r: &ModelRequest) -> u64 {
        self.0.estimate_tokens(r)
    }
    fn complete(&self, r: &ModelRequest) -> bogovolution_core::Result<ModelResponse> {
        self.0.complete(r)
    }
}

#[test]
fn a_local_http_server_that_dies_mid_run_leaves_no_run_behind() {
    let (_d, repo) = project(false);
    set_config(
        &repo,
        "max_tokens_per_run = 200000",
        "max_tokens_per_run = 10000000",
    );
    let (port, server) = fake_local_server(7);
    let settings = LocalSettings {
        url: format!("http://127.0.0.1:{port}"),
        timeout_secs: 10,
        ..LocalSettings::default()
    };
    let model = ViaHttp(LocalModel::new(&settings).unwrap());
    let opts = RunOptions {
        version: "root".into(),
        repeats: Some(2),
        base_seed: 7,
    };
    // 5 tasks per run: run 0 completes on calls 1 to 5, run 1 dies on call 8.
    let err = run_version(&repo, &model, &opts).unwrap_err();
    server.join().unwrap();
    assert!(matches!(err, Error::ModelUnavailable(_)), "{err}");
    assert!(err.to_string().contains("run 1 of version"), "{err}");
    let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
    let root = tree.node(&tree.root).unwrap();
    assert_eq!(root.runs.len(), 1, "only the whole run 0 is recorded");
    assert_eq!(root.runs[0].passed, 5);
    // The server comes back: the same command records run 1 only.
    let (port2, server2) = fake_local_server(5);
    let settings2 = LocalSettings {
        url: format!("http://127.0.0.1:{port2}"),
        ..settings
    };
    let again = run_version(&repo, &ViaHttp(LocalModel::new(&settings2).unwrap()), &opts).unwrap();
    server2.join().unwrap();
    assert_eq!(again.skipped_existing, vec![0]);
    assert_eq!(again.runs.len(), 1);
    assert_eq!(again.runs[0].passed, 5);
    log_ok(&repo);
}

// ------------------------------------------------------------- (c) float medians

#[test]
fn a_verdict_with_a_long_decimal_median_verifies_and_the_log_keeps_growing() {
    // 24 tasks, 5 then 10 passing: medians of 5/24 (0.20833333333333334)
    // and 10/24 (0.4166666666666667), numbers that made verify-log call a
    // valid log edited before v2.7.1.
    let (_d, repo) = project(false);
    let mut tasks = String::from("[sandbox]\nhidden = [\"check.sh\"]\n");
    let mut cfg = String::from("seed = 2\nbase_pass_probability = 0.0\ntokens_per_call = 100\n");
    for (marker, from, to) in [("FIVE", 0, 5), ("TEN", 5, 10)] {
        let ids: Vec<String> = (from..to).map(|i| format!("\"t{i}\"")).collect();
        cfg.push_str(&format!(
            "[[marker]]\ntext = \"{marker}\"\ndelta = 1.0\ntasks = [{}]\n",
            ids.join(", ")
        ));
    }
    for i in 0..24 {
        tasks.push_str(&format!(
            "[[task]]\nid = \"t{i}\"\nprompt = \"p t{i}\"\ncheck = \"sh check.sh t{i}\"\n"
        ));
        cfg.push_str(&format!(
            "[[answer]]\ntask = \"t{i}\"\npath = \"answers/t{i}.txt\"\npass_content = \"ok\\n\"\nfail_content = \"no\\n\"\n"
        ));
    }
    std::fs::write(repo.store().root().join("tasks.toml"), tasks).unwrap();
    set_config(
        &repo,
        "max_tokens_per_run = 200000",
        "max_tokens_per_run = 10000000",
    );
    let mut m = Scripted::new(
        false,
        vec![
            Ok(("base rules\nFIVE\n", "five")),
            Ok(("base rules\nFIVE\nTEN\n", "ten")),
        ],
    );
    m.inner = MockModel::new(toml::from_str::<MockConfig>(&cfg).unwrap());
    evolve_n(&repo, &m, 2).unwrap();
    let medians: Vec<f64> = repo
        .events_of(Some(EventKind::Verdict))
        .unwrap()
        .iter()
        .flat_map(|e| match Payload::from_event(e).unwrap() {
            Payload::Verdict(v) => vec![v.parent_median, v.child_median],
            _ => vec![],
        })
        .flatten()
        .collect();
    assert!(medians.contains(&(5.0 / 24.0)), "{medians:?}");
    assert!(medians.contains(&(10.0 / 24.0)), "{medians:?}");
    log_ok(&repo);
    assert!(repo.recover_interrupted_write().unwrap().is_none());
}

// ---------------------------------------------------------------- (d) token cap

#[test]
fn a_capped_run_stops_the_version_says_why_and_cannot_be_judged() {
    let (_d, repo) = project(false);
    // about 100 tokens a call: 250 fits two of the five tasks
    set_config(
        &repo,
        "max_tokens_per_run = 200000",
        "max_tokens_per_run = 250",
    );
    let m = mock(false);
    let report = run_version(
        &repo,
        &m,
        &RunOptions {
            version: "root".into(),
            repeats: None,
            base_seed: 7,
        },
    )
    .unwrap();
    assert_eq!(report.runs.len(), 1, "no runs after the capped one");
    let note = report.stopped_at_token_cap.expect("a note");
    assert!(note.contains("hit the token cap"), "{note}");
    assert!(note.contains("3 of 5 tasks were not done"), "{note}");
    assert!(note.contains("budget.max_tokens_per_run"), "{note}");

    let cfg = repo.config().unwrap();
    let child = propose(&repo, &cfg, "root", b"base rules\nGENERAL\n", "general")
        .unwrap()
        .child
        .unwrap();
    run_version(
        &repo,
        &m,
        &RunOptions {
            version: child.clone(),
            repeats: Some(1),
            base_seed: 7,
        },
    )
    .unwrap();
    set_config(&repo, "repeats = 5", "repeats = 1");
    set_config(&repo, "min_paired_wins = 4", "min_paired_wins = 1");
    set_config(
        &repo,
        "held_out_min_paired_wins = 3",
        "held_out_min_paired_wins = 1",
    );
    let err = verify_child(&repo, &child).unwrap_err().to_string();
    assert!(err.contains("stopped at the token cap"), "{err}");
    assert_eq!(kinds(&repo, EventKind::Verdict), 0);
}

#[test]
fn evolve_stops_with_the_cap_note_instead_of_judging_capped_runs() {
    let (_d, repo) = project(false);
    set_config(
        &repo,
        "max_tokens_per_run = 200000",
        "max_tokens_per_run = 250",
    );
    let m = Scripted::new(false, vec![Ok(("base rules\nGENERAL\n", "general"))]);
    let err = evolve_n(&repo, &m, 1).unwrap_err().to_string();
    assert!(err.contains("hit the token cap"), "{err}");
    assert!(m.seeds.borrow().is_empty(), "no mutation was asked for");
    assert_eq!(kinds(&repo, EventKind::Verdict), 0);
}

// ----------------------------------------------------- (e) cut off log writes

fn log_paths(repo: &Repo) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = repo.store().root();
    (root.join("log.jsonl"), root.join("HEAD"))
}

fn root_runs(repo: &Repo, model: &dyn Model, repeats: u32) {
    run_version(
        repo,
        model,
        &RunOptions {
            version: "root".into(),
            repeats: Some(repeats),
            base_seed: 7,
        },
    )
    .unwrap();
}

#[test]
fn a_half_written_last_line_is_moved_aside_and_the_run_resumes() {
    let (_d, repo) = project(false);
    let m = mock(false);
    root_runs(&repo, &m, 2);
    let (log, _) = log_paths(&repo);
    let text = std::fs::read_to_string(&log).unwrap();
    // kill in the middle of writing run 2: half a line, no newline
    let fake_line = text
        .lines()
        .last()
        .unwrap()
        .replace("\"run_index\":1", "\"run_index\":2");
    let half = &fake_line[..fake_line.len() / 2];
    std::fs::write(&log, format!("{text}{half}")).unwrap();
    assert!(!repo.verify().unwrap().chain.valid);
    let report = run_version(
        &repo,
        &m,
        &RunOptions {
            version: "root".into(),
            repeats: Some(3),
            base_seed: 7,
        },
    )
    .unwrap();
    assert!(report.recovered_log.unwrap().contains("incomplete bytes"));
    assert_eq!(report.skipped_existing, vec![0, 1]);
    assert_eq!(report.runs.len(), 1);
    let side = std::fs::read_to_string(log.with_extension("jsonl.partial")).unwrap();
    assert!(side.starts_with(half));
    log_ok(&repo);
}

#[test]
fn head_one_event_behind_is_brought_up_to_date() {
    let (_d, repo) = project(false);
    let m = mock(false);
    root_runs(&repo, &m, 1);
    let (_, head) = log_paths(&repo);
    let head_after_one = std::fs::read_to_string(&head).unwrap();
    root_runs(&repo, &m, 2);
    // killed after the run was written, before HEAD was updated
    std::fs::write(&head, &head_after_one).unwrap();
    assert!(!repo.verify().unwrap().chain.valid);
    let report = run_version(
        &repo,
        &m,
        &RunOptions {
            version: "root".into(),
            repeats: Some(3),
            base_seed: 7,
        },
    )
    .unwrap();
    assert!(report
        .recovered_log
        .unwrap()
        .contains("HEAD was one event behind"));
    assert_eq!(report.skipped_existing, vec![0, 1]);
    log_ok(&repo);
}

#[test]
fn a_whole_last_event_without_its_newline_gets_one() {
    let (_d, repo) = project(false);
    let m = mock(false);
    root_runs(&repo, &m, 1);
    let (log, _) = log_paths(&repo);
    let text = std::fs::read_to_string(&log).unwrap();
    std::fs::write(&log, text.trim_end_matches('\n')).unwrap();
    root_runs(&repo, &m, 2);
    log_ok(&repo);
    assert_eq!(kinds(&repo, EventKind::RunRecorded), 2);
}

#[test]
fn tampering_is_not_repaired() {
    let (_d, repo) = project(false);
    let m = mock(false);
    root_runs(&repo, &m, 2);
    let (log, _) = log_paths(&repo);
    let text = std::fs::read_to_string(&log).unwrap();
    // an edited event in the middle, and a cut off HEAD two events back
    std::fs::write(&log, text.replacen("\"passed\":", "\"passed\":1", 1)).unwrap();
    assert!(repo.recover_interrupted_write().unwrap().is_none());
    assert!(!repo.verify().unwrap().chain.valid);
    assert!(run_version(
        &repo,
        &m,
        &RunOptions {
            version: "root".into(),
            repeats: Some(3),
            base_seed: 7,
        },
    )
    .is_err());
}

#[test]
fn head_two_events_behind_is_not_repaired() {
    let (_d, repo) = project(false);
    let m = mock(false);
    root_runs(&repo, &m, 1);
    let (_, head) = log_paths(&repo);
    let old = std::fs::read_to_string(&head).unwrap();
    root_runs(&repo, &m, 3);
    std::fs::write(&head, old).unwrap();
    assert!(repo.recover_interrupted_write().unwrap().is_none());
    assert!(!repo.verify().unwrap().chain.valid);
}

// ------------------------------------------------------ (f) positive controls

#[test]
fn positive_control_a_general_rule_is_accepted_and_a_narrow_one_is_overfit() {
    let (_d, repo) = project(true);
    let m = Scripted::new(
        true,
        vec![
            // lifts only the train tasks: must be overfit, not accepted
            Ok(("base rules\nNARROW\n", "a narrow hint")),
            // lifts every task, train and held out: must be accepted
            Ok(("base rules\nGENERAL\n", "a general hint")),
            // changes no task: must not be accepted
            Ok(("base rules\nGENERAL\nbe tidy\n", "a cosmetic hint")),
        ],
    );
    let report = evolve_n(&repo, &m, 3).unwrap();
    assert_eq!(
        verdicts(&repo),
        vec![
            VerdictKind::Overfit,
            VerdictKind::Accepted,
            VerdictKind::Rejected
        ]
    );
    assert_eq!(
        report.generations[0].current, report.start,
        "overfit did not move it"
    );
    let accepted = report.generations[1].mutation.result.child.clone().unwrap();
    assert_eq!(
        report.generations[1].current, accepted,
        "current moved on yes"
    );
    assert_eq!(report.end, accepted);
    let narrow = &report.generations[0].verdict.as_ref().unwrap().decision;
    assert!(narrow.gain_pp >= 10.0, "the narrow rule did win on train");
    let held = &report.generations[1]
        .verdict
        .as_ref()
        .unwrap()
        .decision
        .held_out;
    assert!(held.as_ref().unwrap().gain_pp >= 10.0);
    log_ok(&repo);
}

#[test]
fn generations_that_change_nothing_never_move_current() {
    let (_d, repo) = project(true);
    let m = Scripted::new(
        true,
        vec![
            Ok(("base rules\nuse clear names\n", "names")),
            Ok(("base rules\nwrite small functions\n", "functions")),
            Ok(("base rules\nadd comments\n", "comments")),
        ],
    );
    let report = evolve_n(&repo, &m, 3).unwrap();
    assert_eq!(report.accepted, 0);
    assert_eq!(report.end, report.start);
    assert!(verdicts(&repo)
        .iter()
        .all(|v| matches!(v, VerdictKind::Rejected | VerdictKind::Noise)));
    log_ok(&repo);
}

#[test]
fn an_unfinished_generation_after_a_crash_is_finished_not_duplicated() {
    // A crash right after the mutation was logged: the child exists, no runs.
    let (_d, repo) = project(true);
    root_runs(&repo, &mock(true), 5);
    let cfg = repo.config().unwrap();
    let child = propose(&repo, &cfg, "root", b"base rules\nGENERAL\n", "general")
        .unwrap()
        .child
        .unwrap();
    let m = Scripted::new(true, vec![]);
    let report = evolve_n(&repo, &m, 1).unwrap();
    assert!(report.generations[0].resumed);
    assert_eq!(
        report.generations[0].mutation.result.child.as_deref(),
        Some(child.as_str())
    );
    assert!(m.seeds.borrow().is_empty());
    assert_eq!(kinds(&repo, EventKind::MutationProposed), 1);
    assert_eq!(verdicts(&repo), vec![VerdictKind::Accepted]);
    log_ok(&repo);
}
