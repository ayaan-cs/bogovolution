//! Real escape attempts against each isolation backend that works on this
//! machine.
//!
//! A backend that cannot start here (not installed, no user namespaces, no
//! daemon, no image) is skipped with a loud "SKIPPED" message on stderr (run
//! with `--nocapture` to see it). Nothing is faked: a skipped backend ran no
//! escape attempt. Set `RULES_EVOLVE_REQUIRE_BACKENDS=bwrap,docker,podman` (any
//! subset) to turn a skip into a failure, for example in CI.

use rules_evolve_core::isolation::{
    probe_backend, Backend, CheckStatus, Isolation, Protected, Runner, SandboxSettings,
};
use rules_evolve_core::model::ProviderKind;
use std::net::TcpListener;
use std::path::Path;
use std::time::{Duration, Instant};

/// `Runner::run` with nothing protected, for the general escape tests.
trait Go {
    fn go(
        &self,
        dir: &Path,
        check: &str,
        env: &[(String, String)],
        timeout: Duration,
    ) -> rules_evolve_core::Result<CheckStatus>;
}

impl Go for Runner {
    fn go(
        &self,
        dir: &Path,
        check: &str,
        env: &[(String, String)],
        timeout: Duration,
    ) -> rules_evolve_core::Result<CheckStatus> {
        self.run(dir, check, env, &Protected::default(), timeout)
    }
}

fn settings(i: Isolation) -> SandboxSettings {
    SandboxSettings {
        isolation: i,
        ..SandboxSettings::default()
    }
}

/// The settings used by the real tests, with small limits.
fn limited(i: Isolation) -> SandboxSettings {
    SandboxSettings {
        isolation: i,
        memory_mb: 256,
        max_processes: 64,
        max_file_mb: 1,
        ..SandboxSettings::default()
    }
}

fn runner_or_skip(backend: Backend, s: SandboxSettings) -> Option<Runner> {
    match probe_backend(backend, &s) {
        Ok(()) => Some(Runner::resolve(&s, ProviderKind::Real).expect("probe passed")),
        Err(why) => {
            let required = std::env::var("RULES_EVOLVE_REQUIRE_BACKENDS").unwrap_or_default();
            if required.split(',').any(|b| b.trim() == backend.as_str()) {
                panic!(
                    "{} is required but does not work here: {why}",
                    backend.as_str()
                );
            }
            eprintln!(
                "SKIPPED: no escape tests ran for {} because it does not work here: {why}",
                backend.as_str()
            );
            None
        }
    }
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap_or_default()
}

const T: Duration = Duration::from_secs(60);

/// Host processes whose command line contains `marker`.
fn processes_with(marker: &str) -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if let Ok(c) = std::fs::read(e.path().join("cmdline")) {
                if String::from_utf8_lossy(&c).contains(marker) {
                    n += 1;
                }
            }
        }
    }
    n
}

fn wait_until_gone(marker: &str) -> usize {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let n = processes_with(marker);
        if n == 0 || Instant::now() > end {
            return n;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn network_is_blocked(runner: &Runner, none: &Runner) {
    // A listener on the host's loopback. With no network namespace a check can
    // reach it. With one, its own loopback is empty.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let script = format!(
        "if command -v python3 >/dev/null 2>&1; then \
           python3 -c 'import socket; s=socket.socket(); s.settimeout(3); s.connect((\"127.0.0.1\", {port}))' 2>/dev/null \
             && echo connected > net || echo blocked > net; \
         elif command -v nc >/dev/null 2>&1; then \
           nc -z -w 3 127.0.0.1 {port} && echo connected > net || echo blocked > net; \
         elif command -v wget >/dev/null 2>&1; then \
           wget -q -T 3 -O /dev/null http://127.0.0.1:{port}/ 2>/dev/null; \
           [ $? -eq 4 ] && echo blocked > net || echo connected > net; \
         else echo notool > net; fi"
    );
    let d = tempfile::tempdir().unwrap();
    runner.go(d.path(), &script, &[], T).unwrap();
    let got = read(d.path(), "net");
    assert_eq!(got.trim(), "blocked", "the check reached the host network");

    // Control: without isolation the same check reaches the listener, so the
    // result above means something. Skipped when the host has no client tool.
    let c = tempfile::tempdir().unwrap();
    none.go(c.path(), &script, &[], T).unwrap();
    let control = read(c.path(), "net");
    if control.trim() == "notool" {
        eprintln!("note: no python3, nc or wget on the host, so no network control ran");
    } else {
        assert_eq!(
            control.trim(),
            "connected",
            "control check could not connect"
        );
    }
    drop(listener);
}

fn writes_outside_the_worktree_fail(runner: &Runner) {
    let uniq = format!("re-esc-{}", std::process::id());
    let d = tempfile::tempdir().unwrap();
    // Paths a check might try to change. All of them are outside the worktree.
    // A real folder on the host that is not under /tmp (a sandbox /tmp is its
    // own scratch space, so a path there says nothing).
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&target).unwrap();
    let outside = tempfile::tempdir_in(&target).unwrap();
    let host_parent = outside.path().join(&uniq);
    let script = format!(
        "for p in /usr/{uniq} /etc/{uniq} /{uniq} /bin/{uniq} /work/../{uniq} {parent}; do \
           if (echo x > \"$p\") 2>/dev/null; then echo \"WROTE $p\" >> res; else echo denied >> res; fi; \
         done; \
         echo inside > inside; \
         echo tmp > /tmp/{uniq} && cat /tmp/{uniq} > tmpres",
        parent = host_parent.display()
    );
    runner.go(d.path(), &script, &[], T).unwrap();
    let res = read(d.path(), "res");
    assert!(!res.contains("WROTE"), "a write escaped:\n{res}");
    assert_eq!(res.lines().count(), 6, "{res}");
    // The worktree itself is writable.
    assert_eq!(read(d.path(), "inside").trim(), "inside");
    // /tmp is private scratch: usable inside, absent on the host.
    assert_eq!(read(d.path(), "tmpres").trim(), "tmp");
    assert!(!Path::new(&format!("/tmp/{uniq}")).exists());
    assert!(!host_parent.exists(), "a write reached the host folder");
    for p in [
        format!("/usr/{uniq}"),
        format!("/etc/{uniq}"),
        format!("/{uniq}"),
        host_parent.display().to_string(),
    ] {
        assert!(!Path::new(&p).exists(), "{p} appeared on the host");
    }
}

fn hidden_files_are_not_readable(runner: &Runner, none: &Runner) {
    // The real repo, with a hidden benchmark file in it.
    let repo = tempfile::tempdir().unwrap();
    let secret = repo.path().join("secret-test.sh");
    std::fs::write(&secret, "the hidden benchmark\n").unwrap();
    let worktree = tempfile::tempdir().unwrap();
    let script = format!(
        "if cat '{p}' > got 2>/dev/null; then echo READ > res; else echo denied > res; fi; \
         if ls '{r}' > /dev/null 2>&1; then echo LISTED >> res; else echo denied >> res; fi; \
         if ls '{w}' > /dev/null 2>&1; then echo LISTED >> res; else echo denied >> res; fi",
        p = secret.display(),
        r = repo.path().display(),
        w = worktree.path().display()
    );
    runner.go(worktree.path(), &script, &[], T).unwrap();
    assert_eq!(
        read(worktree.path(), "res").trim(),
        "denied\ndenied\ndenied"
    );
    assert_eq!(read(worktree.path(), "got"), "");

    // Control: with no isolation the same check reads the real file.
    let c = tempfile::tempdir().unwrap();
    none.go(
        c.path(),
        &script.replace(
            &worktree.path().display().to_string(),
            &c.path().display().to_string(),
        ),
        &[],
        T,
    )
    .unwrap();
    assert!(read(c.path(), "res").starts_with("READ"));
    assert_eq!(read(c.path(), "got"), "the hidden benchmark\n");
}

fn environment_is_cleared(runner: &Runner) {
    let secret_name = format!("RULES_EVOLVE_TEST_SECRET_{}", std::process::id());
    std::env::set_var(&secret_name, "do-not-leak");
    let d = tempfile::tempdir().unwrap();
    let pass = [("RULES_EVOLVE_PASSED".to_string(), "yes".to_string())];
    runner.go(d.path(), "env > envout", &pass, T).unwrap();
    std::env::remove_var(&secret_name);
    let out = read(d.path(), "envout");
    assert!(
        !out.contains("do-not-leak") && !out.contains(&secret_name),
        "{out}"
    );
    assert!(out.contains("RULES_EVOLVE_PASSED=yes"), "{out}");
}

fn limits_and_timeouts_hold(runner: &Runner, backend: Backend) {
    let d = tempfile::tempdir().unwrap();

    // A fork bomb ends at the timeout and leaves nothing behind.
    let marker = format!("RE_BOMB_{}_{}", backend.as_str(), std::process::id());
    let bomb = format!("# {marker}\nbomb() {{ bomb | bomb & }}; bomb; wait");
    let t = Instant::now();
    let st = runner
        .go(d.path(), &bomb, &[], Duration::from_secs(6))
        .unwrap();
    // It may end on its own (forks start failing at the cap) or at the timeout.
    // Either way it is contained.
    assert!(matches!(
        st,
        CheckStatus::Passed | CheckStatus::Failed | CheckStatus::TimedOut
    ));
    assert!(
        t.elapsed() < Duration::from_secs(40),
        "the bomb was not stopped"
    );
    assert_eq!(
        wait_until_gone(&marker),
        0,
        "fork bomb processes were left running"
    );
    // The sandbox still works afterwards.
    assert_eq!(
        runner.go(d.path(), "true", &[], T).unwrap(),
        CheckStatus::Passed
    );

    // The process limit is real: with a cap of 64, a loop that starts 200
    // sleepers cannot get them all. The loop runs in a subshell because a
    // shell exits when a fork fails, and it logs every pid it got (a builtin,
    // so logging needs no process of its own).
    runner
        .go(
            d.path(),
            "(i=0; while [ $i -lt 200 ]; do sleep 20 & echo $! >> pids; i=$((i+1)); done) 2>/dev/null",
            &[],
            Duration::from_secs(20),
        )
        .unwrap();
    let started = read(d.path(), "pids").lines().count();
    assert!(
        (20..64).contains(&started),
        "{started} processes started under a cap of 64"
    );

    // A long process hits the timeout and is killed.
    let marker = format!("RE_SLEEP_{}_{}", backend.as_str(), std::process::id());
    let t = Instant::now();
    let st = runner
        .go(
            d.path(),
            &format!("# {marker}\nsleep 60"),
            &[],
            Duration::from_secs(1),
        )
        .unwrap();
    assert_eq!(st, CheckStatus::TimedOut);
    assert!(t.elapsed() < Duration::from_secs(30));
    assert_eq!(
        wait_until_gone(&marker),
        0,
        "the long process was left running"
    );

    // A child that outlives the shell is killed too.
    let marker = format!("RE_BG_{}_{}", backend.as_str(), std::process::id());
    let st = runner
        .go(
            d.path(),
            &format!("# {marker}\nsleep 60 & sleep 60"),
            &[],
            Duration::from_secs(1),
        )
        .unwrap();
    assert_eq!(st, CheckStatus::TimedOut);
    assert_eq!(
        wait_until_gone(&marker),
        0,
        "a background child was left running"
    );

    // Memory: a 1 GiB buffer is refused under a 256 MiB limit, a small one is fine.
    let big = runner
        .go(
            d.path(),
            "dd if=/dev/zero of=/dev/null bs=1G count=1 2>/dev/null",
            &[],
            T,
        )
        .unwrap();
    assert_eq!(big, CheckStatus::Failed, "the memory limit did not apply");
    let small = runner
        .go(
            d.path(),
            "dd if=/dev/zero of=/dev/null bs=1M count=1 2>/dev/null",
            &[],
            T,
        )
        .unwrap();
    assert_eq!(small, CheckStatus::Passed);

    // File size: 1 MiB is the most one file may grow to.
    let _ = runner
        .go(
            d.path(),
            "dd if=/dev/zero of=big bs=1M count=10 2>/dev/null",
            &[],
            T,
        )
        .unwrap();
    let size = std::fs::metadata(d.path().join("big"))
        .map(|m| m.len())
        .unwrap_or(0);
    assert!(size <= 1024 * 1024, "a file grew to {size} bytes");
}

fn full_suite(backend: Backend, iso: Isolation) {
    let Some(runner) = runner_or_skip(backend, limited(iso)) else {
        return;
    };
    assert_eq!(runner.backend(), backend);
    let none = Runner::unsafe_none();
    network_is_blocked(&runner, &none);
    writes_outside_the_worktree_fail(&runner);
    hidden_files_are_not_readable(&runner, &none);
    environment_is_cleared(&runner);
    limits_and_timeouts_hold(&runner, backend);
}

#[test]
fn bwrap_blocks_escapes_and_enforces_limits() {
    full_suite(Backend::Bwrap, Isolation::Bwrap);
}

#[test]
fn bwrap_shows_only_the_system_and_the_worktree() {
    let Some(runner) = runner_or_skip(Backend::Bwrap, settings(Isolation::Bwrap)) else {
        return;
    };
    let d = tempfile::tempdir().unwrap();
    runner
        .go(
            d.path(),
            "ls / > rootls; ls /work > worklist; id -u > uid",
            &[],
            T,
        )
        .unwrap();
    let allowed = [
        "bin", "dev", "etc", "lib", "lib32", "lib64", "libx32", "opt", "proc", "sbin", "snap",
        "tmp", "usr", "work",
    ];
    for entry in read(d.path(), "rootls").lines() {
        assert!(
            allowed.contains(&entry),
            "unexpected {entry:?} in the sandbox root"
        );
    }
    assert!(!read(d.path(), "rootls")
        .lines()
        .any(|l| l == "home" || l == "root" || l == "var"));
    // The worktree is what /work holds.
    assert!(read(d.path(), "worklist").contains("rootls"));
    // Not root inside either (the user namespace maps your own id).
    assert_ne!(read(d.path(), "uid").trim(), "0");
}

#[test]
fn docker_blocks_escapes_and_enforces_limits() {
    full_suite(Backend::Docker, Isolation::Docker);
}

#[test]
fn podman_blocks_escapes_and_enforces_limits() {
    full_suite(Backend::Podman, Isolation::Podman);
}

#[test]
fn a_backend_that_is_not_there_gives_a_clear_error_for_a_real_provider() {
    // This one runs everywhere: with no working isolation a real provider is
    // refused, never silently run unsafe.
    let mut s = settings(Isolation::Auto);
    s.image = "example.invalid/definitely-not-pulled:0".into();
    let r = Runner::resolve(&s, ProviderKind::Real);
    match r {
        Ok(runner) => assert!(runner.backend().is_isolated()),
        Err(e) => assert!(e.to_string().contains("no isolation backend works")),
    }
    // And `none` is refused for a real provider no matter what is installed.
    let err = Runner::resolve(&settings(Isolation::None), ProviderKind::Real).unwrap_err();
    assert!(err.to_string().contains("real provider must run isolated"));
}
