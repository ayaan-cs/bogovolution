//! Operating system isolation for the check command.
//!
//! The check command, and any code the model wrote that the check pulls in
//! (a build script, a test config), runs through a [`Runner`]. A runner is one
//! of four backends:
//!
//! * `bwrap` (bubblewrap): Linux, no daemon. New user, pid, net, ipc and uts
//!   namespaces, a read only system, the worktree as the only writable bind,
//!   `/tmp` on tmpfs, a cleared environment, die with parent.
//! * `docker` and `podman`: `--network none`, read only root, tmpfs `/tmp`, a
//!   non-root user, all capabilities dropped, no-new-privileges, and process,
//!   memory and cpu limits. Only the worktree is mounted.
//! * `none`: no isolation. Kept for dev and for the mock and replay providers.
//!   It is unsafe and is always labeled that way.
//! * `auto` (a setting, not a backend): the strongest backend that works here.
//!
//! The argument lists are built by pure functions ([`build_command`]) so they
//! can be unit tested without any backend installed.
//!
//! Policy: a provider whose kind is [`ProviderKind::Real`] may never run with
//! `none`. [`Runner::resolve`] enforces that, and `run_version` and `mutate`
//! both go through it, so a new provider cannot skip it.

use crate::error::{Error, Result};
use crate::shadow::ShadowMode;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::time::{Duration, Instant};

/// Image used by `docker` and `podman` unless the config names another.
pub const DEFAULT_IMAGE: &str = "docker.io/library/alpine:3.20";

/// `PATH` inside `bwrap`. Only system folders are mounted there.
pub const SANDBOX_PATH: &str = "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

/// Where the worktree appears inside `bwrap` and containers.
pub const WORK_DIR: &str = "/work";

/// Label written next to `none` wherever it is shown.
pub const UNSAFE_LABEL: &str = "none (UNSAFE: no operating system isolation)";

/// What the config asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Isolation {
    /// The strongest backend that works here.
    #[default]
    Auto,
    Bwrap,
    Docker,
    Podman,
    /// No isolation. Unsafe. Mock and replay providers only.
    None,
}

impl Isolation {
    pub fn as_str(self) -> &'static str {
        match self {
            Isolation::Auto => "auto",
            Isolation::Bwrap => "bwrap",
            Isolation::Docker => "docker",
            Isolation::Podman => "podman",
            Isolation::None => "none",
        }
    }
}

impl FromStr for Isolation {
    type Err = Error;
    fn from_str(s: &str) -> Result<Isolation> {
        match s {
            "auto" => Ok(Isolation::Auto),
            "bwrap" => Ok(Isolation::Bwrap),
            "docker" => Ok(Isolation::Docker),
            "podman" => Ok(Isolation::Podman),
            "none" => Ok(Isolation::None),
            other => Err(Error::Input(format!(
                "unknown isolation {other:?}. Use auto, bwrap, docker, podman or none"
            ))),
        }
    }
}

/// A backend that was actually chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Bwrap,
    Docker,
    Podman,
    None,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Bwrap => "bwrap",
            Backend::Docker => "docker",
            Backend::Podman => "podman",
            Backend::None => "none",
        }
    }

    /// True for every backend except `none`.
    pub fn is_isolated(self) -> bool {
        self != Backend::None
    }

    /// The program that has to be on `PATH`.
    pub fn program(self) -> &'static str {
        match self {
            Backend::Bwrap => "bwrap",
            Backend::Docker => "docker",
            Backend::Podman => "podman",
            Backend::None => "sh",
        }
    }

    /// Text for people: `none` always carries the unsafe label.
    pub fn label(self) -> &'static str {
        match self {
            Backend::None => UNSAFE_LABEL,
            other => other.as_str(),
        }
    }
}

/// How much a provider can be trusted not to need isolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// Mock and replay: deterministic, local, no network, no tools.
    Local,
    /// A real or network provider. Must run isolated.
    Real,
    /// A real model on this machine (`local` provider). It costs nothing and
    /// only talks to localhost, but what it writes is real model output that
    /// the checks then run, so it must run isolated like `Real`.
    LocalModel,
}

impl ProviderKind {
    /// Whether checks for this kind of provider must run in an isolation backend.
    pub fn needs_isolation(self) -> bool {
        matches!(self, ProviderKind::Real | ProviderKind::LocalModel)
    }
}

/// The `[sandbox]` table of `config.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SandboxSettings {
    /// `auto`, `bwrap`, `docker`, `podman` or `none`.
    pub isolation: Isolation,
    /// Container image for `docker` and `podman`.
    pub image: String,
    /// Memory limit for one check, in MiB.
    pub memory_mb: u64,
    /// Cpu limit for one check (`docker` and `podman` only).
    pub cpus: f64,
    /// Most processes one check may have.
    pub max_processes: u64,
    /// Largest single file one check may write, in MiB.
    pub max_file_mb: u64,
    /// Extra host folders mounted read only, for example a toolchain.
    pub ro_binds: Vec<String>,
    /// What to do about layouts where model-written files could shadow hidden
    /// tests: `warn` (default), `error` or `off`.
    pub shadow_warnings: ShadowMode,
}

impl Default for SandboxSettings {
    fn default() -> Self {
        SandboxSettings {
            isolation: Isolation::Auto,
            image: DEFAULT_IMAGE.to_string(),
            memory_mb: 2048,
            cpus: 2.0,
            max_processes: 256,
            max_file_mb: 256,
            ro_binds: Vec::new(),
            shadow_warnings: ShadowMode::Warn,
        }
    }
}

impl SandboxSettings {
    pub fn validate(&self) -> Result<()> {
        let bad = |m: String| Err(Error::Input(format!("[sandbox] in config.toml: {m}")));
        if self.memory_mb < 64 {
            return bad("memory_mb must be at least 64".into());
        }
        if !(self.cpus.is_finite() && self.cpus >= 0.1) {
            return bad("cpus must be at least 0.1".into());
        }
        if self.max_processes < 8 {
            return bad("max_processes must be at least 8".into());
        }
        if self.max_file_mb < 1 {
            return bad("max_file_mb must be at least 1".into());
        }
        let img = &self.image;
        if img.is_empty() || img.starts_with('-') || img.chars().any(|c| c.is_whitespace()) {
            return bad(format!("image {img:?} is not a valid image name"));
        }
        for p in &self.ro_binds {
            if !p.starts_with('/') || p.contains([',', ':', '\0', '\n']) || p.contains("/../") {
                return bad(format!(
                    "ro_binds entry {p:?} must be an absolute path without ',' ':' or '..'"
                ));
            }
        }
        Ok(())
    }
}

/// A command ready to start: what to run, with which environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    /// The only environment the launcher process gets.
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    /// Run this after a timeout, to stop work the launcher cannot reach
    /// (a container keeps running when its client is killed).
    pub cleanup: Option<(String, Vec<String>)>,
}

/// Who owns the worktree. Containers run as this user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostIds {
    pub uid: u32,
    pub gid: u32,
}

/// One path inside the worktree that must show the original content and stay
/// read only during the check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadOnlyMount {
    /// Path relative to the worktree, with `/` separators.
    pub rel: String,
    /// The pristine copy on the host, outside the worktree. A file or a folder.
    pub source: PathBuf,
}

/// What the backend must protect inside the worktree while the check runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Protected {
    /// Folders (relative, shallowest first) that hold a protected path. They are
    /// bound onto themselves so they cannot be renamed, moved or replaced by a
    /// link while the check runs: a mount point cannot be renamed or removed.
    pub pin_dirs: Vec<String>,
    /// Read only mounts of the pristine copies, over the worktree paths.
    pub read_only: Vec<ReadOnlyMount>,
}

impl Protected {
    pub fn is_empty(&self) -> bool {
        self.pin_dirs.is_empty() && self.read_only.is_empty()
    }

    /// Container volumes are written `src:dst:ro`, so a path with `:` or `,`
    /// cannot be passed safely. Refuse it rather than guess.
    fn check_container_paths(&self, worktree: &Path) -> Result<()> {
        let bad = |t: &str| t.contains([':', ',', '\n', '\0']);
        let wt = worktree.to_string_lossy();
        let mut paths: Vec<String> = vec![wt.to_string()];
        paths.extend(self.pin_dirs.iter().cloned());
        for m in &self.read_only {
            paths.push(m.rel.clone());
            paths.push(m.source.to_string_lossy().to_string());
        }
        match paths.into_iter().find(|p| bad(p)) {
            Some(p) => Err(Error::Input(format!(
                "the path {p:?} has a ':' or ',' in it, which docker and podman volumes cannot hold safely. Rename it, or use isolation = \"bwrap\""
            ))),
            None => Ok(()),
        }
    }
}

fn mib(n: u64) -> u64 {
    n * 1024 * 1024
}

/// System folders that `bwrap` mounts read only when they exist.
const BWRAP_SYSTEM: [&str; 9] = [
    "/usr", "/bin", "/sbin", "/lib", "/lib32", "/lib64", "/libx32", "/opt", "/snap",
];

/// Single files and folders from `/etc` that tools need. Nothing else of
/// `/etc` is visible.
const BWRAP_ETC: [&str; 6] = [
    "/etc/alternatives",
    "/etc/ld.so.cache",
    "/etc/ld.so.conf",
    "/etc/ld.so.conf.d",
    "/etc/ssl",
    "/etc/localtime",
];

/// What one check needs, besides the backend and settings.
#[derive(Debug, Clone)]
pub struct CheckRequest<'a> {
    pub worktree: &'a Path,
    pub check: &'a str,
    /// The passthrough variables from `tasks.toml`, after lookup.
    pub env: &'a [(String, String)],
    /// Paths that must show their original content and stay read only.
    pub protected: &'a Protected,
    /// The host `PATH`, used only to find `bwrap`, `docker`, `podman` or `sh`.
    pub launcher_path: Option<&'a str>,
    /// A unique container name.
    pub container: &'a str,
    pub ids: HostIds,
}

/// Build the command that runs a check with `backend`.
pub fn build_command(backend: Backend, s: &SandboxSettings, req: &CheckRequest) -> CommandSpec {
    let CheckRequest {
        worktree,
        check,
        env,
        protected,
        launcher_path,
        container,
        ids,
    } = *req;
    let wt = worktree.to_string_lossy().to_string();
    let mut launcher_env = Vec::new();
    if let Some(p) = launcher_path {
        launcher_env.push(("PATH".to_string(), p.to_string()));
    }
    match backend {
        Backend::None => {
            let mut e = vec![("HOME".to_string(), wt)];
            if let Some(p) = launcher_path {
                e.push(("PATH".to_string(), p.to_string()));
            }
            e.extend(env.iter().cloned());
            CommandSpec {
                program: "sh".into(),
                args: vec!["-c".into(), check.into()],
                env: e,
                cwd: Some(worktree.to_path_buf()),
                cleanup: None,
            }
        }
        Backend::Bwrap => {
            let mut a: Vec<String> = Vec::new();
            let mut push = |items: &[&str]| a.extend(items.iter().map(|x| x.to_string()));
            push(&[
                "--die-with-parent",
                "--new-session",
                "--unshare-user",
                "--unshare-pid",
                "--unshare-net",
                "--unshare-ipc",
                "--unshare-uts",
                "--cap-drop",
                "ALL",
                "--clearenv",
                "--hostname",
                "rules-evolve",
            ]);
            for p in BWRAP_SYSTEM {
                push(&["--ro-bind-try", p, p]);
            }
            for p in BWRAP_ETC {
                push(&["--ro-bind-try", p, p]);
            }
            for p in &s.ro_binds {
                push(&["--ro-bind", p, p]);
            }
            let tmp_size = mib(256).to_string();
            push(&[
                "--proc", "/proc", "--dev", "/dev", "--size", &tmp_size, "--tmpfs", "/tmp",
                "--bind", &wt, WORK_DIR,
            ]);
            // Pin every folder that holds a protected path, then lay the
            // pristine copies over their paths, read only. Parents come first.
            for rel in &protected.pin_dirs {
                let src = worktree.join(rel).to_string_lossy().to_string();
                push(&["--bind", &src, &format!("{WORK_DIR}/{rel}")]);
            }
            for m in &protected.read_only {
                let src = m.source.to_string_lossy().to_string();
                push(&["--ro-bind", &src, &format!("{WORK_DIR}/{}", m.rel)]);
            }
            push(&[
                "--remount-ro",
                "/",
                "--chdir",
                WORK_DIR,
                "--setenv",
                "PATH",
                SANDBOX_PATH,
                "--setenv",
                "HOME",
                WORK_DIR,
                "--setenv",
                "TMPDIR",
                "/tmp",
            ]);
            for (k, v) in env {
                push(&["--setenv", k, v]);
            }
            // Limits are applied inside the new user namespace, so the process
            // count is only about the sandbox and not every process you run.
            let nproc = format!("--nproc={}", s.max_processes);
            let address_space = format!("--as={}", mib(s.memory_mb));
            let fsize = format!("--fsize={}", mib(s.max_file_mb));
            push(&[
                "prlimit",
                &nproc,
                &address_space,
                &fsize,
                "--nofile=1024",
                "--core=0",
                "--",
                "sh",
                "-c",
                check,
            ]);
            CommandSpec {
                program: "bwrap".into(),
                args: a,
                env: launcher_env,
                cwd: None,
                cleanup: None,
            }
        }
        Backend::Docker | Backend::Podman => {
            let runtime = backend.as_str();
            let user = format!("{}:{}", ids.uid, ids.gid);
            let mut a: Vec<String> = Vec::new();
            let mut push = |items: &[&str]| a.extend(items.iter().map(|x| x.to_string()));
            push(&["run", "--rm", "--name", container, "--network", "none"]);
            push(&[
                "--read-only",
                "--tmpfs",
                "/tmp:rw,nosuid,nodev,size=256m",
                "--cap-drop",
                "ALL",
                "--security-opt",
                "no-new-privileges",
                "--ipc",
                "none",
                "--pull",
                "never",
                "--hostname",
                "rules-evolve",
            ]);
            if backend == Backend::Podman {
                // Rootless podman maps your user to root unless told to keep
                // the id, and then the worktree would not be writable.
                push(&["--userns", "keep-id"]);
                // Rootless podman without cgroup delegation (no systemd user
                // session, for example in a container) silently drops
                // --pids-limit, --memory and --cpus. Resource limits still
                // hold there, as with bwrap: RLIMIT_NPROC counts per user
                // namespace, which rootless podman always creates, and
                // RLIMIT_DATA caps each process's heap and private mappings
                // (podman has no `as` ulimit). Found by the escape test in
                // v2.6.0. CPU has no such fallback.
                let nproc = format!("nproc={0}:{0}", s.max_processes);
                let data = format!("data={0}:{0}", mib(s.memory_mb));
                push(&["--ulimit", &nproc, "--ulimit", &data]);
            }
            push(&["--user", &user]);
            let pids = s.max_processes.to_string();
            let mem = format!("{}m", s.memory_mb);
            let cpus = format!("{}", s.cpus);
            let nofile = "nofile=1024:1024";
            let fsize = format!("fsize={0}:{0}", mib(s.max_file_mb));
            push(&[
                "--pids-limit",
                &pids,
                "--memory",
                &mem,
                "--memory-swap",
                &mem,
                "--cpus",
                &cpus,
                "--ulimit",
                nofile,
                "--ulimit",
                &fsize,
                "--ulimit",
                "core=0",
            ]);
            push(&["--env", &format!("HOME={WORK_DIR}")]);
            push(&["--env", "TMPDIR=/tmp"]);
            for (k, v) in env {
                push(&["--env", &format!("{k}={v}")]);
            }
            push(&["--volume", &format!("{wt}:{WORK_DIR}:rw")]);
            for rel in &protected.pin_dirs {
                let src = worktree.join(rel).to_string_lossy().to_string();
                push(&["--volume", &format!("{src}:{WORK_DIR}/{rel}:rw")]);
            }
            for m in &protected.read_only {
                let src = m.source.to_string_lossy().to_string();
                push(&["--volume", &format!("{src}:{WORK_DIR}/{}:ro", m.rel)]);
            }
            for p in &s.ro_binds {
                push(&["--volume", &format!("{p}:{p}:ro")]);
            }
            push(&["--workdir", WORK_DIR, &s.image, "sh", "-c", check]);
            CommandSpec {
                program: runtime.into(),
                args: a,
                env: launcher_env,
                cwd: None,
                cleanup: Some((
                    runtime.to_string(),
                    vec!["rm".into(), "-f".into(), container.into()],
                )),
            }
        }
    }
}

/// Does a program exist on `PATH`?
fn find_in_path(name: &str, path: Option<&str>) -> Option<PathBuf> {
    // `split_paths` uses `:` on Unix and `;` on Windows, where programs also
    // end in `.exe`.
    let names: Vec<String> = if cfg!(windows) {
        vec![format!("{name}.exe"), name.to_string()]
    } else {
        vec![name.to_string()]
    };
    std::env::split_paths(path?)
        .filter(|d| !d.as_os_str().is_empty())
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|p| p.is_file())
}

/// How a command ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Passed,
    Failed,
    TimedOut,
}

fn quiet(cmd: &mut Command) -> &mut Command {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
}

/// Stop everything a timed out command started.
fn kill_all(spec: &CommandSpec, child: &mut std::process::Child) {
    if let Some((prog, args)) = &spec.cleanup {
        let _ = quiet(Command::new(prog).args(args)).status();
    }
    // The child leads its own process group, so this reaches what it forked.
    // `kill -9 -PGID` works in dash and bash; dash rejects `kill -KILL -- -PGID`.
    #[cfg(unix)]
    {
        let group = format!("kill -9 -{} 2>/dev/null; true", child.id());
        let _ = quiet(Command::new("sh").arg("-c").arg(group)).status();
    }
    // Windows has no process groups here: stop the whole process tree.
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let _ = quiet(Command::new("taskkill").args(["/T", "/F", "/PID", &pid])).status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The child leads its own process group, so a timeout can stop all of it.
#[cfg(unix)]
fn own_process_group(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

#[cfg(not(unix))]
fn own_process_group(_cmd: &mut Command) {}

/// The environment is cleared for every command. On Windows a few system
/// variables are put back, without which programs cannot even open a
/// socket (`SystemRoot`) or find their settings (`docker` reads its context
/// from `USERPROFILE`). They hold paths, not secrets. Nothing on Unix.
fn windows_system_env() -> Vec<(String, String)> {
    if !cfg!(windows) {
        return Vec::new();
    }
    const KEEP: [&str; 14] = [
        "SystemRoot",
        "SystemDrive",
        "windir",
        "ComSpec",
        "PATHEXT",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "ProgramData",
        "ProgramFiles",
        "DOCKER_HOST",
        "DOCKER_CONTEXT",
    ];
    KEEP.iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
        .collect()
}

/// Start a command and wait for it, up to `timeout`. With `capture`, the last
/// part of stderr is returned (used for the startup probe).
pub fn run_spec(
    spec: &CommandSpec,
    timeout: Duration,
    capture: bool,
) -> Result<(CheckStatus, String)> {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .env_clear()
        .envs(windows_system_env())
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    own_process_group(&mut cmd);
    cmd.stderr(if capture {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    let mut child = cmd.spawn().map_err(|e| {
        Error::Input(format!(
            "could not start the check command ({}): {e}{}",
            spec.program,
            start_hint(&spec.program)
        ))
    })?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut text = String::new();
                if let Some(mut err) = child.stderr.take() {
                    let mut buf = Vec::new();
                    let _ = err.by_ref().take(8192).read_to_end(&mut buf);
                    text = String::from_utf8_lossy(&buf).trim().to_string();
                }
                let st = if status.success() {
                    CheckStatus::Passed
                } else {
                    CheckStatus::Failed
                };
                return Ok((st, text));
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    kill_all(spec, &mut child);
                    return Ok((CheckStatus::TimedOut, String::new()));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(Error::Input(format!("check command failed: {e}"))),
        }
    }
}

/// Most bytes kept from the end of a check's output (stdout and stderr each).
pub const CHECK_OUTPUT_KEEP: usize = 8192;

/// Read a pipe to the end on its own thread and keep only the last `keep`
/// bytes, so a chatty check can never block on a full pipe or fill memory.
fn tail_reader<R: Read + Send + 'static>(
    mut pipe: R,
    keep: usize,
) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut kept: std::collections::VecDeque<u8> = std::collections::VecDeque::new();
        let mut buf = [0u8; 4096];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    kept.extend(&buf[..n]);
                    let extra = kept.len().saturating_sub(keep);
                    kept.drain(..extra);
                }
            }
        }
        let _ = tx.send(kept.into_iter().collect());
    });
    rx
}

/// Like [`run_spec`], but stdout and stderr are both kept (the last
/// [`CHECK_OUTPUT_KEEP`] bytes of each) and returned as one text: stdout first,
/// then stderr. A timed out command returns what it printed before it was
/// stopped.
pub fn run_spec_captured(spec: &CommandSpec, timeout: Duration) -> Result<(CheckStatus, String)> {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .env_clear()
        .envs(windows_system_env())
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    own_process_group(&mut cmd);
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    let mut child = cmd.spawn().map_err(|e| {
        Error::Input(format!(
            "could not start the check command ({}): {e}{}",
            spec.program,
            start_hint(&spec.program)
        ))
    })?;
    let out = child
        .stdout
        .take()
        .map(|p| tail_reader(p, CHECK_OUTPUT_KEEP));
    let err = child
        .stderr
        .take()
        .map(|p| tail_reader(p, CHECK_OUTPUT_KEEP));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    CheckStatus::Passed
                } else {
                    CheckStatus::Failed
                }
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    kill_all(spec, &mut child);
                    break CheckStatus::TimedOut;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(Error::Input(format!("check command failed: {e}"))),
        }
    };
    // A process the check left behind may still hold a pipe open. Do not wait
    // for it forever: take what arrived within a moment.
    let collect = |rx: Option<std::sync::mpsc::Receiver<Vec<u8>>>| {
        rx.and_then(|r| r.recv_timeout(Duration::from_secs(2)).ok())
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_default()
    };
    let (o, e) = (collect(out), collect(err));
    let mut text = o;
    if !e.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&e);
    }
    Ok((status, text))
}

fn unique_name() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!(
        "rules-evolve-{}-{}-{nanos}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

/// The user a container runs as. Never root: a root owner becomes "nobody".
pub fn ids_for_owner(uid: u32, gid: u32) -> HostIds {
    if uid == 0 {
        HostIds {
            uid: 65534,
            gid: 65534,
        }
    } else {
        HostIds { uid, gid }
    }
}

#[cfg(unix)]
fn host_ids(worktree: &Path) -> HostIds {
    use std::os::unix::fs::MetadataExt;
    match std::fs::metadata(worktree) {
        Ok(m) => ids_for_owner(m.uid(), m.gid()),
        Err(_) => ids_for_owner(0, 0),
    }
}

/// Windows files have no Unix owner. Docker Desktop and Podman shares let
/// any user in the Linux VM write, so the container runs as "nobody".
#[cfg(not(unix))]
fn host_ids(_worktree: &Path) -> HostIds {
    ids_for_owner(0, 0)
}

#[cfg(unix)]
fn open_for_nobody(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::symlink_metadata(dir) {
        if meta.file_type().is_symlink() {
            return;
        }
        let mode = if meta.is_dir() { 0o777 } else { 0o666 };
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode));
        if meta.is_dir() {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    open_for_nobody(&e.path());
                }
            }
        }
    }
}

/// Runs check commands with one backend.
#[derive(Debug, Clone)]
pub struct Runner {
    backend: Backend,
    settings: SandboxSettings,
}

impl Runner {
    /// No isolation. Unsafe. Only for dev, tests and the mock and replay
    /// providers. [`Runner::resolve`] never returns this for a real provider.
    pub fn unsafe_none() -> Runner {
        Runner {
            backend: Backend::None,
            settings: SandboxSettings::default(),
        }
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    pub fn settings(&self) -> &SandboxSettings {
        &self.settings
    }

    /// Choose a backend for `settings`, check the policy for `kind`, and make
    /// sure the backend really starts on this machine.
    pub fn resolve(settings: &SandboxSettings, kind: ProviderKind) -> Result<Runner> {
        Runner::resolve_with(settings, kind, &|b, s| probe_backend(b, s))
    }

    /// Like [`Runner::resolve`] with the startup check replaced, so tests can
    /// say which backends "work".
    pub fn resolve_with(
        settings: &SandboxSettings,
        kind: ProviderKind,
        probe: &dyn Fn(Backend, &SandboxSettings) -> std::result::Result<(), String>,
    ) -> Result<Runner> {
        settings.validate()?;
        check_policy(kind, settings.isolation)?;
        let backend = match settings.isolation {
            Isolation::None => Backend::None,
            Isolation::Bwrap | Isolation::Docker | Isolation::Podman => {
                let b = match settings.isolation {
                    Isolation::Bwrap => Backend::Bwrap,
                    Isolation::Docker => Backend::Docker,
                    _ => Backend::Podman,
                };
                probe(b, settings).map_err(|why| {
                    Error::Input(format!(
                        "isolation = {:?} was asked for but it does not work here: {why}",
                        b.as_str()
                    ))
                })?;
                b
            }
            Isolation::Auto => {
                let mut why = Vec::new();
                let mut found = None;
                for b in AUTO_ORDER {
                    match probe(b, settings) {
                        Ok(()) => {
                            found = Some(b);
                            break;
                        }
                        Err(w) => why.push(format!("{}: {w}", b.as_str())),
                    }
                }
                match (found, kind) {
                    (Some(b), _) => b,
                    (None, ProviderKind::Local) => Backend::None,
                    (None, ProviderKind::Real | ProviderKind::LocalModel) => {
                        return Err(Error::Input(format!(
                            "no isolation backend works here, and a real provider must run isolated. {} Tried: {}",
                            install_hint(),
                            why.join("; ")
                        )))
                    }
                }
            }
        };
        enforce_resolved(kind, backend)?;
        Ok(Runner {
            backend,
            settings: settings.clone(),
        })
    }

    /// Run `check` with this backend in `worktree`.
    pub fn run(
        &self,
        worktree: &Path,
        check: &str,
        env: &[(String, String)],
        protected: &Protected,
        timeout: Duration,
    ) -> Result<CheckStatus> {
        if matches!(self.backend, Backend::Docker | Backend::Podman) {
            protected.check_container_paths(worktree)?;
        }
        let spec = self.spec(worktree, check, env, protected);
        if matches!(self.backend, Backend::Docker | Backend::Podman) {
            open_for_nobody_if_root(worktree);
        }
        run_spec(&spec, timeout, false).map(|(s, _)| s)
    }

    /// Like [`Runner::run`], and also returns the tail of what the check
    /// printed (stdout, then stderr).
    pub fn run_captured(
        &self,
        worktree: &Path,
        check: &str,
        env: &[(String, String)],
        protected: &Protected,
        timeout: Duration,
    ) -> Result<(CheckStatus, String)> {
        if matches!(self.backend, Backend::Docker | Backend::Podman) {
            protected.check_container_paths(worktree)?;
        }
        let spec = self.spec(worktree, check, env, protected);
        if matches!(self.backend, Backend::Docker | Backend::Podman) {
            open_for_nobody_if_root(worktree);
        }
        run_spec_captured(&spec, timeout)
    }

    /// The command that [`Runner::run`] would start.
    pub fn spec(
        &self,
        worktree: &Path,
        check: &str,
        env: &[(String, String)],
        protected: &Protected,
    ) -> CommandSpec {
        let path = std::env::var("PATH").ok();
        build_command(
            self.backend,
            &self.settings,
            &CheckRequest {
                worktree,
                check,
                env,
                protected,
                launcher_path: path.as_deref(),
                container: &unique_name(),
                ids: host_ids(worktree),
            },
        )
    }
}

#[cfg(unix)]
fn open_for_nobody_if_root(worktree: &Path) {
    use std::os::unix::fs::MetadataExt;
    if std::fs::metadata(worktree)
        .map(|m| m.uid() == 0)
        .unwrap_or(false)
    {
        open_for_nobody(worktree);
    }
}

#[cfg(not(unix))]
fn open_for_nobody_if_root(_worktree: &Path) {}

/// Extra help when a check command cannot start.
fn start_hint(program: &str) -> &'static str {
    if cfg!(windows) && program == "sh" {
        ". On Windows, checks without isolation run with sh: install Git for Windows and put its usr\\bin folder (it has sh.exe) on PATH, or use WSL2"
    } else {
        ""
    }
}

/// What to install when no backend works.
pub fn install_hint() -> &'static str {
    if cfg!(windows) {
        "On Windows, install and start Docker Desktop (or Podman Desktop), or run the Linux build of rules-evolve inside WSL2. bubblewrap is Linux only."
    } else if cfg!(target_os = "macos") {
        "Install and start Docker Desktop or Podman. bubblewrap is Linux only."
    } else {
        "Install bubblewrap, podman or docker."
    }
}

/// The order `auto` tries. `bwrap` is first because it uses the host
/// toolchain, needs no image and no daemon, so checks that work today keep
/// working. Containers are used when it cannot start.
pub const AUTO_ORDER: [Backend; 3] = [Backend::Bwrap, Backend::Podman, Backend::Docker];

/// The part of the policy that needs no probing: a real provider may not ask
/// for `none`.
pub fn check_policy(kind: ProviderKind, asked: Isolation) -> Result<()> {
    if kind.needs_isolation() && asked == Isolation::None {
        return Err(Error::Input(
            "a real provider must run isolated: set isolation to auto, bwrap, docker or podman in [sandbox] of config.toml (isolation = \"none\" is for the mock and replay providers only)"
                .into(),
        ));
    }
    Ok(())
}

fn enforce_resolved(kind: ProviderKind, backend: Backend) -> Result<()> {
    if kind.needs_isolation() && !backend.is_isolated() {
        return Err(Error::Input(
            "a real provider must run isolated, but no isolation backend was chosen".into(),
        ));
    }
    Ok(())
}

/// Start `true` with a backend and report why it failed, if it did.
pub fn probe_backend(
    backend: Backend,
    settings: &SandboxSettings,
) -> std::result::Result<(), String> {
    if backend == Backend::None {
        return Ok(());
    }
    if backend == Backend::Bwrap && !cfg!(target_os = "linux") {
        return Err("bubblewrap runs only on Linux".into());
    }
    let path = std::env::var("PATH").ok();
    if find_in_path(backend.program(), path.as_deref()).is_none() {
        return Err(format!("{} is not on PATH", backend.program()));
    }
    let dir = tempfile::Builder::new()
        .prefix("rules-evolve-probe-")
        .tempdir()
        .map_err(|e| format!("could not make a temp folder: {e}"))?;
    let runner = Runner {
        backend,
        settings: settings.clone(),
    };
    if matches!(backend, Backend::Docker | Backend::Podman) {
        open_for_nobody_if_root(dir.path());
    }
    let spec = runner.spec(dir.path(), "true", &[], &Protected::default());
    match run_spec(&spec, Duration::from_secs(90), true) {
        Ok((CheckStatus::Passed, _)) => Ok(()),
        Ok((CheckStatus::TimedOut, _)) => Err("starting it took more than 90 seconds".into()),
        Ok((CheckStatus::Failed, err)) => {
            let first = err.lines().next().unwrap_or("it exited with an error");
            let hint = if matches!(backend, Backend::Docker | Backend::Podman) {
                format!(
                    " (the image {:?} must already exist locally, runs never pull)",
                    settings.image
                )
            } else {
                String::new()
            };
            Err(format!("{first}{hint}"))
        }
        Err(e) => Err(e.to_string()),
    }
}

/// One backend, as shown by `rules-evolve isolation`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BackendStatus {
    pub backend: Backend,
    pub works: bool,
    /// Why it does not work, when it does not.
    pub problem: Option<String>,
}

/// Probe every backend and say which one `auto` would pick.
pub fn survey(settings: &SandboxSettings) -> (Vec<BackendStatus>, Option<Backend>) {
    let mut out = Vec::new();
    let mut auto = None;
    for b in AUTO_ORDER {
        let r = probe_backend(b, settings);
        if r.is_ok() && auto.is_none() {
            auto = Some(b);
        }
        out.push(BackendStatus {
            backend: b,
            works: r.is_ok(),
            problem: r.err(),
        });
    }
    (out, auto)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDS: HostIds = HostIds {
        uid: 1000,
        gid: 1000,
    };

    fn spec(backend: Backend, s: &SandboxSettings) -> CommandSpec {
        spec_with(backend, s, &Protected::default())
    }

    fn spec_with(backend: Backend, s: &SandboxSettings, protected: &Protected) -> CommandSpec {
        build_command(
            backend,
            s,
            &CheckRequest {
                worktree: Path::new("/tmp/wt-1"),
                check: "sh check.sh task-01",
                env: &[("FOO".to_string(), "bar".to_string())],
                protected,
                launcher_path: Some("/usr/bin:/bin"),
                container: "re-test",
                ids: IDS,
            },
        )
    }

    fn has_pair(args: &[String], a: &str, b: &str) -> bool {
        args.windows(2).any(|w| w[0] == a && w[1] == b)
    }

    fn has_triple(args: &[String], a: &str, b: &str, c: &str) -> bool {
        args.windows(3).any(|w| w[0] == a && w[1] == b && w[2] == c)
    }

    // ---- argument construction (always runs) ----

    #[test]
    fn bwrap_arguments_isolate_everything_and_mount_only_the_worktree() {
        let s = SandboxSettings {
            ro_binds: vec!["/opt/tools".into()],
            ..SandboxSettings::default()
        };
        let c = spec(Backend::Bwrap, &s);
        assert_eq!(c.program, "bwrap");
        let a = &c.args;
        for flag in [
            "--die-with-parent",
            "--new-session",
            "--unshare-user",
            "--unshare-pid",
            "--unshare-net",
            "--unshare-ipc",
            "--unshare-uts",
            "--clearenv",
        ] {
            assert!(a.iter().any(|x| x == flag), "missing {flag}");
        }
        assert!(has_pair(a, "--cap-drop", "ALL"));
        assert!(has_pair(a, "--proc", "/proc"));
        assert!(has_pair(a, "--dev", "/dev"));
        assert!(has_pair(a, "--tmpfs", "/tmp"));
        assert!(has_pair(a, "--remount-ro", "/"));
        assert!(has_pair(a, "--chdir", "/work"));
        // The worktree is the only writable bind, and it is the only --bind.
        assert_eq!(a.iter().filter(|x| *x == "--bind").count(), 1);
        assert!(has_triple(a, "--bind", "/tmp/wt-1", "/work"));
        // The system comes in read only. The whole root and home never do.
        assert!(has_triple(a, "--ro-bind-try", "/usr", "/usr"));
        assert!(!has_triple(a, "--ro-bind", "/", "/"));
        assert!(!a.iter().any(|x| x.starts_with("/home") || x == "/root"));
        assert!(has_triple(a, "--ro-bind", "/opt/tools", "/opt/tools"));
        // Environment: cleared, with a fixed PATH and HOME inside.
        assert!(has_triple(a, "--setenv", "PATH", SANDBOX_PATH));
        assert!(has_triple(a, "--setenv", "HOME", "/work"));
        assert!(has_triple(a, "--setenv", "FOO", "bar"));
        // Limits go inside the new user namespace, right before the command.
        let n = a.len();
        assert_eq!(&a[n - 3..], ["sh", "-c", "sh check.sh task-01"]);
        assert_eq!(a[n - 4], "--");
        let limits = &a[n - 10..n - 4];
        assert_eq!(limits[0], "prlimit");
        assert!(limits.iter().any(|x| x == "--nproc=256"));
        assert!(limits.iter().any(|x| x == "--as=2147483648"));
        assert!(limits.iter().any(|x| x == "--fsize=268435456"));
        // The launcher only gets PATH, to find bwrap.
        assert_eq!(
            c.env,
            vec![("PATH".to_string(), "/usr/bin:/bin".to_string())]
        );
        assert_eq!(c.cwd, None);
        assert_eq!(c.cleanup, None);
    }

    fn container_checks(backend: Backend) -> CommandSpec {
        let s = SandboxSettings {
            image: "example.invalid/img:1".into(),
            memory_mb: 512,
            cpus: 1.5,
            max_processes: 99,
            max_file_mb: 7,
            ro_binds: vec!["/opt/tools".into()],
            ..SandboxSettings::default()
        };
        let c = spec(backend, &s);
        let a = &c.args;
        let rt = backend.as_str();
        assert_eq!(c.program, rt);
        assert_eq!(a[0], "run");
        assert!(a.iter().any(|x| x == "--rm"));
        assert!(has_pair(a, "--name", "re-test"));
        assert!(has_pair(a, "--network", "none"));
        assert!(a.iter().any(|x| x == "--read-only"));
        assert!(has_pair(a, "--tmpfs", "/tmp:rw,nosuid,nodev,size=256m"));
        assert!(has_pair(a, "--cap-drop", "ALL"));
        assert!(has_pair(a, "--security-opt", "no-new-privileges"));
        assert!(has_pair(a, "--ipc", "none"));
        assert!(has_pair(a, "--pull", "never"));
        assert!(has_pair(a, "--user", "1000:1000"));
        assert!(has_pair(a, "--pids-limit", "99"));
        assert!(has_pair(a, "--memory", "512m"));
        assert!(has_pair(a, "--memory-swap", "512m"));
        assert!(has_pair(a, "--cpus", "1.5"));
        assert!(has_pair(a, "--ulimit", "fsize=7340032:7340032"));
        assert!(has_pair(a, "--env", "HOME=/work"));
        assert!(has_pair(a, "--env", "FOO=bar"));
        // Only the worktree is writable. Extra mounts are read only.
        let vols: Vec<&String> = a
            .iter()
            .enumerate()
            .filter(|(_, x)| *x == "--volume")
            .map(|(i, _)| &a[i + 1])
            .collect();
        assert_eq!(vols, vec!["/tmp/wt-1:/work:rw", "/opt/tools:/opt/tools:ro"]);
        assert!(!a.iter().any(|x| x == "--privileged"));
        assert!(!a.iter().any(|x| x.starts_with("--network=host")));
        // The image, then the command, are last.
        let n = a.len();
        assert_eq!(
            a[n - 6..],
            [
                "--workdir",
                "/work",
                "example.invalid/img:1",
                "sh",
                "-c",
                "sh check.sh task-01"
            ]
        );
        assert_eq!(
            c.cleanup,
            Some((
                rt.to_string(),
                vec!["rm".into(), "-f".into(), "re-test".into()]
            ))
        );
        c
    }

    #[test]
    fn docker_arguments_follow_the_policy() {
        let c = container_checks(Backend::Docker);
        assert!(!c.args.iter().any(|x| x == "--userns"));
        // rootful docker shares the host user's process count, so no nproc
        assert!(!c.args.iter().any(|x| x.starts_with("nproc=")));
    }

    #[test]
    fn podman_arguments_follow_the_policy_and_keep_the_user_id() {
        let c = container_checks(Backend::Podman);
        assert!(has_pair(&c.args, "--userns", "keep-id"));
        // the process cap also holds where rootless podman drops cgroup limits
        assert!(has_pair(&c.args, "--ulimit", "nproc=99:99"));
        let mem = mib(512);
        assert!(has_pair(&c.args, "--ulimit", &format!("data={mem}:{mem}")));
    }

    #[test]
    fn containers_never_run_as_root() {
        assert_eq!(
            ids_for_owner(1000, 100),
            HostIds {
                uid: 1000,
                gid: 100
            }
        );
        let root = ids_for_owner(0, 0);
        assert_eq!(
            root,
            HostIds {
                uid: 65534,
                gid: 65534
            }
        );
    }

    #[test]
    fn none_runs_sh_with_a_cleared_environment() {
        let c = spec(Backend::None, &SandboxSettings::default());
        assert_eq!(c.program, "sh");
        assert_eq!(c.args, vec!["-c", "sh check.sh task-01"]);
        assert_eq!(c.cwd.as_deref(), Some(Path::new("/tmp/wt-1")));
        let names: Vec<&str> = c.env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, vec!["HOME", "PATH", "FOO"]);
    }

    #[test]
    fn none_is_always_labeled_unsafe() {
        assert!(Backend::None.label().contains("UNSAFE"));
        assert_eq!(Backend::Bwrap.label(), "bwrap");
        assert!(!Backend::None.is_isolated());
        assert!(Backend::Docker.is_isolated());
    }

    #[test]
    fn isolation_names_round_trip() {
        for i in [
            Isolation::Auto,
            Isolation::Bwrap,
            Isolation::Docker,
            Isolation::Podman,
            Isolation::None,
        ] {
            assert_eq!(i.as_str().parse::<Isolation>().unwrap(), i);
        }
        assert!("chroot".parse::<Isolation>().is_err());
    }

    #[test]
    fn settings_are_validated() {
        assert!(SandboxSettings::default().validate().is_ok());
        let bad = |f: &dyn Fn(&mut SandboxSettings)| {
            let mut s = SandboxSettings::default();
            f(&mut s);
            s.validate().is_err()
        };
        assert!(bad(&|s| s.memory_mb = 1));
        assert!(bad(&|s| s.cpus = 0.0));
        assert!(bad(&|s| s.cpus = f64::NAN));
        assert!(bad(&|s| s.max_processes = 1));
        assert!(bad(&|s| s.max_file_mb = 0));
        assert!(bad(&|s| s.image = "".into()));
        assert!(bad(&|s| s.image = "--privileged".into()));
        assert!(bad(&|s| s.image = "a b".into()));
        assert!(bad(&|s| s.ro_binds = vec!["relative".into()]));
        assert!(bad(&|s| s.ro_binds = vec!["/a:/b".into()]));
        assert!(bad(&|s| s.ro_binds = vec!["/a/../etc".into()]));
    }

    // ---- policy and backend choice (always runs, with a pretend probe) ----

    fn ok_for(
        works: &'static [Backend],
    ) -> impl Fn(Backend, &SandboxSettings) -> std::result::Result<(), String> {
        move |b, _| {
            if works.contains(&b) {
                Ok(())
            } else {
                Err("pretend it is missing".into())
            }
        }
    }

    fn settings(i: Isolation) -> SandboxSettings {
        SandboxSettings {
            isolation: i,
            ..SandboxSettings::default()
        }
    }

    #[test]
    fn a_real_provider_cannot_run_without_isolation() {
        let never = |_: Backend, _: &SandboxSettings| -> std::result::Result<(), String> {
            panic!("the policy must refuse before probing")
        };
        let err = Runner::resolve_with(&settings(Isolation::None), ProviderKind::Real, &never)
            .unwrap_err()
            .to_string();
        assert!(err.contains("real provider must run isolated"), "{err}");
        // Mock and replay may use none.
        let r =
            Runner::resolve_with(&settings(Isolation::None), ProviderKind::Local, &never).unwrap();
        assert_eq!(r.backend(), Backend::None);
    }

    #[test]
    fn a_real_provider_is_refused_when_auto_finds_nothing() {
        let err =
            Runner::resolve_with(&settings(Isolation::Auto), ProviderKind::Real, &ok_for(&[]))
                .unwrap_err()
                .to_string();
        assert!(err.contains("no isolation backend works"), "{err}");
        assert!(err.contains("bwrap") && err.contains("podman") && err.contains("docker"));
        // A local provider falls back to none.
        let r = Runner::resolve_with(
            &settings(Isolation::Auto),
            ProviderKind::Local,
            &ok_for(&[]),
        )
        .unwrap();
        assert_eq!(r.backend(), Backend::None);
    }

    #[test]
    fn a_real_provider_runs_with_any_working_isolated_backend() {
        for (setting, backend) in [
            (Isolation::Bwrap, Backend::Bwrap),
            (Isolation::Docker, Backend::Docker),
            (Isolation::Podman, Backend::Podman),
            (Isolation::Auto, Backend::Podman),
        ] {
            let works: &'static [Backend] = match setting {
                Isolation::Auto => &[Backend::Podman, Backend::Docker],
                Isolation::Bwrap => &[Backend::Bwrap],
                Isolation::Docker => &[Backend::Docker],
                _ => &[Backend::Podman],
            };
            let r = Runner::resolve_with(&settings(setting), ProviderKind::Real, &ok_for(works))
                .unwrap();
            assert_eq!(r.backend(), backend);
        }
    }

    #[test]
    fn auto_prefers_bwrap_then_podman_then_docker() {
        let pick = |works: &'static [Backend]| {
            Runner::resolve_with(
                &settings(Isolation::Auto),
                ProviderKind::Local,
                &ok_for(works),
            )
            .unwrap()
            .backend()
        };
        assert_eq!(
            pick(&[Backend::Docker, Backend::Podman, Backend::Bwrap]),
            Backend::Bwrap
        );
        assert_eq!(pick(&[Backend::Docker, Backend::Podman]), Backend::Podman);
        assert_eq!(pick(&[Backend::Docker]), Backend::Docker);
    }

    #[test]
    fn an_explicit_backend_that_does_not_work_is_an_error_not_a_fallback() {
        for (i, name) in [
            (Isolation::Bwrap, "bwrap"),
            (Isolation::Docker, "docker"),
            (Isolation::Podman, "podman"),
        ] {
            let err = Runner::resolve_with(&settings(i), ProviderKind::Local, &ok_for(&[]))
                .unwrap_err()
                .to_string();
            assert!(
                err.contains(name) && err.contains("does not work here"),
                "{err}"
            );
        }
    }

    #[test]
    fn the_static_policy_check_is_what_mutate_uses() {
        assert!(check_policy(ProviderKind::Real, Isolation::None).is_err());
        for i in [
            Isolation::Auto,
            Isolation::Bwrap,
            Isolation::Docker,
            Isolation::Podman,
        ] {
            assert!(check_policy(ProviderKind::Real, i).is_ok());
        }
        assert!(check_policy(ProviderKind::Local, Isolation::None).is_ok());
    }

    #[test]
    fn find_in_path_looks_only_at_the_given_path() {
        assert!(find_in_path("sh", Some("/usr/bin:/bin")).is_some());
        assert!(find_in_path("sh", Some("/nonexistent")).is_none());
        assert!(find_in_path("sh", None).is_none());
    }

    // ---- protected (hidden) paths ----

    fn protected() -> Protected {
        Protected {
            pin_dirs: vec!["docs".into(), "docs/deep".into()],
            read_only: vec![
                ReadOnlyMount {
                    rel: "check.sh".into(),
                    source: "/pristine/check.sh".into(),
                },
                ReadOnlyMount {
                    rel: "tests".into(),
                    source: "/pristine/tests".into(),
                },
                ReadOnlyMount {
                    rel: "docs/deep/spec.txt".into(),
                    source: "/pristine/docs/deep/spec.txt".into(),
                },
            ],
        }
    }

    /// Positions of a three-part flag in an argument list.
    fn at(args: &[String], a: &str, b: &str, c: &str) -> Option<usize> {
        args.windows(3)
            .position(|w| w[0] == a && w[1] == b && w[2] == c)
    }

    #[test]
    fn bwrap_binds_hidden_paths_read_only_over_themselves_after_the_worktree() {
        let c = spec_with(Backend::Bwrap, &SandboxSettings::default(), &protected());
        let a = &c.args;
        let wt = at(a, "--bind", "/tmp/wt-1", "/work").expect("worktree bind");
        let docs = at(a, "--bind", "/tmp/wt-1/docs", "/work/docs").expect("pin docs");
        let deep = at(a, "--bind", "/tmp/wt-1/docs/deep", "/work/docs/deep").expect("pin deep");
        let check = at(a, "--ro-bind", "/pristine/check.sh", "/work/check.sh").expect("check.sh");
        let tests = at(a, "--ro-bind", "/pristine/tests", "/work/tests").expect("tests");
        let spec = at(
            a,
            "--ro-bind",
            "/pristine/docs/deep/spec.txt",
            "/work/docs/deep/spec.txt",
        )
        .expect("spec");
        // Order matters: worktree, parents (shallow first), then the read only files.
        assert!(wt < docs && docs < deep && deep < check && deep < tests && deep < spec);
        // Everything is mounted before the root is made read only.
        let ro_root = a.iter().position(|x| x == "--remount-ro").unwrap();
        assert!(spec < ro_root);
        // Hidden paths are never mounted writable, and the only writable binds
        // are the worktree and the pinned folders of the worktree.
        for i in a
            .iter()
            .enumerate()
            .filter(|(_, x)| *x == "--bind")
            .map(|(i, _)| i)
        {
            assert!(a[i + 1].starts_with("/tmp/wt-1"), "{:?}", a[i + 1]);
        }
        for m in [
            "/pristine/check.sh",
            "/pristine/tests",
            "/pristine/docs/deep/spec.txt",
        ] {
            let i = a.iter().position(|x| x == m).unwrap();
            assert_eq!(a[i - 1], "--ro-bind");
        }
    }

    #[test]
    fn without_protected_paths_nothing_extra_is_mounted() {
        let c = spec(Backend::Bwrap, &SandboxSettings::default());
        assert_eq!(c.args.iter().filter(|x| *x == "--bind").count(), 1);
        assert_eq!(c.args.iter().filter(|x| *x == "--ro-bind").count(), 0);
    }

    fn volumes(c: &CommandSpec) -> Vec<String> {
        c.args
            .windows(2)
            .filter(|w| w[0] == "--volume")
            .map(|w| w[1].clone())
            .collect()
    }

    #[test]
    fn containers_mount_hidden_paths_with_ro_after_the_worktree() {
        for b in [Backend::Docker, Backend::Podman] {
            let c = spec_with(b, &SandboxSettings::default(), &protected());
            assert_eq!(
                volumes(&c),
                vec![
                    "/tmp/wt-1:/work:rw",
                    "/tmp/wt-1/docs:/work/docs:rw",
                    "/tmp/wt-1/docs/deep:/work/docs/deep:rw",
                    "/pristine/check.sh:/work/check.sh:ro",
                    "/pristine/tests:/work/tests:ro",
                    "/pristine/docs/deep/spec.txt:/work/docs/deep/spec.txt:ro",
                ]
            );
        }
    }

    #[test]
    fn container_paths_that_cannot_be_quoted_are_refused() {
        let mut p = protected();
        p.read_only.push(ReadOnlyMount {
            rel: "we:ird.sh".into(),
            source: "/pristine/we:ird.sh".into(),
        });
        let err = p.check_container_paths(Path::new("/tmp/wt-1")).unwrap_err();
        assert!(err.to_string().contains("docker and podman volumes"));
        let mut q = protected();
        q.pin_dirs.push("a,b".into());
        assert!(q.check_container_paths(Path::new("/tmp/wt-1")).is_err());
        assert!(protected()
            .check_container_paths(Path::new("/tmp/wt-1"))
            .is_ok());
    }

    #[test]
    fn none_does_not_mount_anything_so_writes_are_not_prevented() {
        let c = spec_with(Backend::None, &SandboxSettings::default(), &protected());
        assert_eq!(c.args, vec!["-c", "sh check.sh task-01"]);
    }
}
