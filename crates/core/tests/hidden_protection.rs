//! Hidden test files are read only, and original, while the check runs.
//!
//! A scenario runs the whole path the harness uses: scan the repo, prepare a
//! worktree, let a "model" tamper with it (the things a tool-using model could
//! do before the check), restore the hidden files, then run a check command
//! that is itself hostile code, trying every way it can think of to change what
//! the real check script will run.
//!
//! Backends that cannot start here are skipped with a loud `SKIPPED` line on
//! stderr (run with `--nocapture`). Set
//! `BOGO_REQUIRE_BACKENDS=bwrap,docker,podman` to make a skip a failure.
#![cfg(unix)]

use bogovolution_core::isolation::{probe_backend, Backend, Isolation, Runner, SandboxSettings};
use bogovolution_core::model::ProviderKind;
use bogovolution_core::sandbox::{CheckStatus, Sandbox};
use bogovolution_core::tasks::SandboxConfig;
use std::path::Path;
use std::time::Duration;

fn write(root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_default()
}

const CHECK: &str = "echo REAL > check-ran\n";
const TEST_A: &str = "ORIGINAL TEST A\n";
const TEST_B: &str = "ORIGINAL TEST B\n";
const SPEC: &str = "ORIGINAL SPEC\n";

/// The hostile code. It runs inside the check, before the real check script.
/// It uses only what busybox and dash both have.
const ATTACK: &str = r#"
t() { n=$1; shift; if ( "$@" ) 2>/dev/null; then echo "$n SUCCEEDED" >> res; else echo "$n denied" >> res; fi; }
snap() { cat check.sh > seen_check_$1; cat tests/test_a.py > seen_a_$1; cat tests/sub/test_b.py > seen_b_$1; cat docs/spec.txt > seen_spec_$1; ls tests tests/sub > seen_ls_$1; }

echo "echo EVIL > check-ran" > fake.sh

# Phase 1: change content.
t overwrite        sh -c 'echo "echo EVIL > check-ran" > check.sh'
t append           sh -c 'echo "echo EVIL > check-ran" >> check.sh'
t test_overwrite   sh -c 'echo EVIL > tests/test_a.py'
t nested_overwrite sh -c 'echo EVIL > tests/sub/test_b.py'
t spec_overwrite   sh -c 'echo EVIL > docs/spec.txt'
snap 1

# Phase 2: change what is at the path.
t chmod            chmod 777 check.sh
t delete           rm -f check.sh
t rename           mv check.sh check.sh.bak
t rename_over      mv fake.sh check.sh
t symlink          ln -sf fake.sh check.sh
t hardlink         ln -f fake.sh check.sh
t test_new_file    sh -c 'echo EVIL > tests/conftest.py'
t nested_new_file  sh -c 'echo EVIL > tests/sub/extra.py'
t test_delete      rm tests/test_a.py
t test_rmrf        rm -rf tests
t test_rename      mv tests tests_old
t test_swap        sh -c 'rm -rf tests && ln -s /tmp tests'
t spec_delete      rm docs/spec.txt
t spec_rename      mv docs/spec.txt docs/spec.old
t spec_symlink     ln -sf fake.sh docs/spec.txt
snap 2

# Phase 3: replace a parent folder of a hidden file.
t parent_rename    mv docs docs_old
t parent_swap      sh -c 'rm -rf docs && ln -s /tmp docs'
t parent_to_file   sh -c 'rm -rf docs; echo x > docs'
snap 3

# What the error looks like, and what stays allowed.
( echo EVIL > check.sh ) 2> werr
echo ok > visible-out
echo ok > docs/newfile
# A known gap: a new file next to a hidden file in a folder that is not hidden.
t shadow_beside_hidden_file sh -c 'echo shadow > docs/shadow.py'
"#;

struct Scenario {
    repo: tempfile::TempDir,
    cfg: SandboxConfig,
}

fn scenario() -> Scenario {
    let repo = tempfile::tempdir().unwrap();
    let r = repo.path();
    write(r, "AGENTS.md", "rules\n");
    write(r, "check.sh", CHECK);
    write(r, "tests/test_a.py", TEST_A);
    write(r, "tests/sub/test_b.py", TEST_B);
    write(r, "docs/readme.txt", "visible\n");
    write(r, "docs/spec.txt", SPEC);
    write(r, "attack.sh", ATTACK);
    let cfg = SandboxConfig {
        hidden: vec!["check.sh".into(), "tests".into(), "docs/spec.txt".into()],
        ..SandboxConfig::default()
    };
    Scenario { repo, cfg }
}

fn runner_or_skip(backend: Backend, iso: Isolation) -> Option<Runner> {
    let s = SandboxSettings {
        isolation: iso,
        ..SandboxSettings::default()
    };
    match probe_backend(backend, &s) {
        Ok(()) => Some(Runner::resolve(&s, ProviderKind::Real).expect("probe passed")),
        Err(why) => {
            let required = std::env::var("BOGO_REQUIRE_BACKENDS").unwrap_or_default();
            if required.split(',').any(|b| b.trim() == backend.as_str()) {
                panic!(
                    "{} is required but does not work here: {why}",
                    backend.as_str()
                );
            }
            eprintln!(
                "SKIPPED: no hidden file tests ran for {} because it does not work here: {why}",
                backend.as_str()
            );
            None
        }
    }
}

/// What a tool-using model could do to the worktree before the check starts.
fn tamper_before_check(wt: &Path, outside: &Path) {
    use std::os::unix::fs::symlink;
    // Delete a hidden file, replace a hidden folder by a link to a folder it
    // controls, and replace a parent folder of a hidden file by a link as well.
    std::fs::remove_file(wt.join("check.sh")).ok();
    std::fs::remove_dir_all(wt.join("tests")).ok();
    symlink(outside, wt.join("tests")).unwrap();
    std::fs::remove_dir_all(wt.join("docs")).ok();
    symlink(outside, wt.join("docs")).unwrap();
}

struct Outcome {
    status: CheckStatus,
    wt: tempfile::TempDir,
    outside: tempfile::TempDir,
}

fn run_scenario(sc: &Scenario, runner: Runner) -> Outcome {
    let sandbox = Sandbox::scan(sc.repo.path(), "AGENTS.md", &sc.cfg)
        .unwrap()
        .with_runner(runner);
    let wt = sandbox.prepare(b"rules\n").unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "test_a.py", "EVIL FROM OUTSIDE\n");
    write(outside.path(), "spec.txt", "EVIL SPEC FROM OUTSIDE\n");
    tamper_before_check(wt.path(), outside.path());
    wt.restore_hidden().unwrap();
    let status = wt
        .run_check("sh attack.sh; sh check.sh", Duration::from_secs(120))
        .unwrap();
    // Keep the worktree for inspection after the sandbox is dropped.
    let keep = tempfile::tempdir().unwrap();
    copy_dir(wt.path(), keep.path());
    Outcome {
        status,
        wt: keep,
        outside,
    }
}

fn copy_dir(from: &Path, to: &Path) {
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let meta = std::fs::symlink_metadata(e.path()).unwrap();
        let dest = to.join(e.file_name());
        if meta.is_dir() {
            std::fs::create_dir_all(&dest).unwrap();
            copy_dir(&e.path(), &dest);
        } else if meta.is_file() {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

fn results(o: &Outcome) -> Vec<(String, String)> {
    read(o.wt.path(), "res")
        .lines()
        .map(|l| {
            let (n, r) = l.split_once(' ').unwrap();
            (n.to_string(), r.to_string())
        })
        .collect()
}

const ATTACKS: [&str; 24] = [
    "overwrite",
    "append",
    "test_overwrite",
    "nested_overwrite",
    "spec_overwrite",
    "chmod",
    "delete",
    "rename",
    "rename_over",
    "symlink",
    "hardlink",
    "test_new_file",
    "nested_new_file",
    "test_delete",
    "test_rmrf",
    "test_rename",
    "test_swap",
    "spec_delete",
    "spec_rename",
    "spec_symlink",
    "parent_rename",
    "parent_swap",
    "parent_to_file",
    "shadow_beside_hidden_file",
];

fn isolated_suite(backend: Backend, iso: Isolation) {
    let Some(runner) = runner_or_skip(backend, iso) else {
        return;
    };
    let sc = scenario();
    let o = run_scenario(&sc, runner);
    let res = results(&o);
    let name = backend.as_str();

    // Every attack ran, and every one was refused, except the documented gap.
    for a in ATTACKS.iter().filter(|a| !a.is_empty()) {
        let got: Vec<&str> = res
            .iter()
            .filter(|(n, _)| n == a)
            .map(|(_, r)| r.as_str())
            .collect();
        assert_eq!(got.len(), 1, "{name}: attack {a} did not run: {res:?}");
        if *a == "shadow_beside_hidden_file" {
            // Known residual gap: see the README.
            assert_eq!(got[0], "SUCCEEDED", "{name}: the documented gap changed");
        } else {
            assert_eq!(got[0], "denied", "{name}: attack {a} was not refused");
        }
    }

    // The check sees the original content at every step.
    for phase in ["1", "2", "3"] {
        assert_eq!(
            read(o.wt.path(), &format!("seen_check_{phase}")),
            CHECK,
            "{name} phase {phase}"
        );
        assert_eq!(
            read(o.wt.path(), &format!("seen_a_{phase}")),
            TEST_A,
            "{name} phase {phase}"
        );
        assert_eq!(
            read(o.wt.path(), &format!("seen_b_{phase}")),
            TEST_B,
            "{name} phase {phase}"
        );
        assert_eq!(
            read(o.wt.path(), &format!("seen_spec_{phase}")),
            SPEC,
            "{name} phase {phase}"
        );
    }
    // No file was added to the hidden folders.
    let ls = read(o.wt.path(), "seen_ls_3");
    assert!(!ls.contains("conftest") && !ls.contains("extra.py"), "{ls}");
    // The real check script ran, and it ran the original.
    assert_eq!(
        o.status,
        CheckStatus::Passed,
        "{name}: the real check did not pass"
    );
    assert_eq!(read(o.wt.path(), "check-ran").trim(), "REAL");
    // A write to a hidden file is refused with a read only or permission error.
    let werr = read(o.wt.path(), "werr");
    assert!(
        werr.contains("Read-only file system") || werr.contains("Permission denied"),
        "{name}: unexpected error text {werr:?}"
    );
    // Folders around the hidden files stay usable for the check's own work.
    assert_eq!(read(o.wt.path(), "visible-out").trim(), "ok");
    assert_eq!(read(o.wt.path(), "docs/newfile").trim(), "ok");
    // Nothing was written through the model's links to the folder outside.
    assert_eq!(read(o.outside.path(), "test_a.py"), "EVIL FROM OUTSIDE\n");
    assert_eq!(
        read(o.outside.path(), "spec.txt"),
        "EVIL SPEC FROM OUTSIDE\n"
    );
    assert_eq!(std::fs::read_dir(o.outside.path()).unwrap().count(), 2);
    // The worktree's own copies are untouched too.
    assert_eq!(read(o.wt.path(), "tests/test_a.py"), TEST_A);
    assert_eq!(read(o.wt.path(), "check.sh"), CHECK);
}

#[test]
fn bwrap_keeps_hidden_files_original_and_read_only() {
    isolated_suite(Backend::Bwrap, Isolation::Bwrap);
}

#[test]
fn docker_keeps_hidden_files_original_and_read_only() {
    isolated_suite(Backend::Docker, Isolation::Docker);
}

#[test]
fn podman_keeps_hidden_files_original_and_read_only() {
    isolated_suite(Backend::Podman, Isolation::Podman);
}

#[test]
fn control_under_none_the_same_attacks_succeed() {
    // Without isolation the same hostile code does change what the check runs,
    // so the tests above can fail. `none` restores the pristine copy first, and
    // that is all it does.
    let sc = scenario();
    let o = run_scenario(&sc, Runner::unsafe_none());
    let res = results(&o);
    let ok = |n: &str| res.iter().any(|(a, r)| a == n && r == "SUCCEEDED");
    for a in [
        "overwrite",
        "append",
        "test_overwrite",
        "nested_overwrite",
        "spec_overwrite",
        "delete",
        "test_new_file",
    ] {
        assert!(
            ok(a),
            "control: {a} should succeed with no isolation: {res:?}"
        );
    }
    // What the check saw after phase 1 was already changed.
    assert!(read(o.wt.path(), "seen_check_1").contains("EVIL"));
    assert!(read(o.wt.path(), "seen_a_1").contains("EVIL"));
    assert!(read(o.wt.path(), "seen_spec_1").contains("EVIL"));
    // And the error text from a write is absent: the write worked.
    assert_eq!(read(o.wt.path(), "werr"), "");
    // `none` still removes the model's links before the check, so its writes
    // landed in the worktree and not in the folder the links pointed at.
    assert_eq!(read(o.outside.path(), "test_a.py"), "EVIL FROM OUTSIDE\n");
    assert_eq!(
        read(o.outside.path(), "spec.txt"),
        "EVIL SPEC FROM OUTSIDE\n"
    );
}
