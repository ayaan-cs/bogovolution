//! The `local` provider: a real model running on this machine.
//!
//! It talks to an OpenAI compatible chat endpoint (for example `llama-server`
//! from llama.cpp) over plain HTTP. It is free (no key, no account, no spend)
//! and it can only reach this machine:
//!
//! - the base URL must be `http://` and the host must be exactly `localhost`,
//!   `127.0.0.1` or `[::1]`. Anything else is refused before a socket opens, so
//!   a typo or a pasted cloud URL cannot be billed;
//! - there is no TLS and no DNS lookup. The code connects straight to the
//!   loopback address and checks the peer is a loopback address after connecting;
//! - redirects are never followed.
//!
//! The provider only returns text. It runs no tools. Files it asks for are
//! written by the harness through the same checked `apply_edits` as every other
//! provider, and the checks run in the isolation backend. Because the output is
//! real model output, the provider is [`ProviderKind::LocalModel`]: isolation
//! is required, like for a real provider.
//!
//! The harness prompt is fixed here. The task prompt, the repo files the model
//! may read and the rules text are put in it. The model answers with
//! `FILE: path` followed by a fenced block holding the whole new file.

use crate::error::{Error, Result};
use crate::isolation::ProviderKind;
use crate::model::{
    FileEdit, Model, ModelRequest, ModelResponse, MutationRequest, MutationResponse,
};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

/// Default base URL of a local server.
pub const DEFAULT_URL: &str = "http://127.0.0.1:8080";

/// Largest HTTP response the provider will read.
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
/// Most bytes of repo files shown to the model in one prompt.
const MAX_SNAPSHOT_BYTES: usize = 48 * 1024;
/// Largest single file shown to the model.
const MAX_SNAPSHOT_FILE_BYTES: usize = 16 * 1024;

/// Settings for the `local` provider (command line flags).
#[derive(Debug, Clone, PartialEq)]
pub struct LocalSettings {
    pub url: String,
    pub temperature: f64,
    /// Most tokens the model may write per call.
    pub max_new_tokens: u64,
    /// Whole-call time limit in seconds.
    pub timeout_secs: u64,
    /// Optional JSONL file that gets every raw model reply, for debugging.
    pub transcript: Option<std::path::PathBuf>,
}

impl Default for LocalSettings {
    fn default() -> Self {
        LocalSettings {
            url: DEFAULT_URL.to_string(),
            temperature: 0.7,
            max_new_tokens: 1024,
            timeout_secs: 600,
            transcript: None,
        }
    }
}

/// A checked local endpoint: where to connect and what to put in `Host`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEndpoint {
    pub addr: SocketAddr,
    pub host_header: String,
}

/// Check a base URL against the localhost-only policy.
pub fn parse_local_url(url: &str) -> Result<LocalEndpoint> {
    let refuse = |why: &str| {
        Error::Input(format!(
            "the local provider only talks to this machine: {why} (url: {url:?}). Use http://127.0.0.1:PORT, http://localhost:PORT or http://[::1]:PORT"
        ))
    };
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| refuse("the URL must start with http:// (no https, no other scheme)"))?;
    if rest.is_empty() {
        return Err(refuse("the URL has no host"));
    }
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if !(path.is_empty() || path == "/") {
        return Err(refuse(
            "give only the server address, without a path or query (the /v1/... path is added for you)",
        ));
    }
    if authority.contains('@')
        || authority.contains('\\')
        || authority.contains('?')
        || authority.contains('#')
    {
        return Err(refuse(
            "the address must not hold user info, a query or a fragment",
        ));
    }
    let (host, port_text) = if let Some(stripped) = authority.strip_prefix('[') {
        let end = stripped
            .find(']')
            .ok_or_else(|| refuse("unclosed [ in the address"))?;
        let host = &stripped[..end];
        let after = &stripped[end + 1..];
        let port = match after {
            "" => None,
            p => Some(p.strip_prefix(':').ok_or_else(|| refuse("bad port"))?),
        };
        (format!("[{host}]"), port)
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (authority.to_string(), None),
        }
    };
    let port: u16 = match port_text {
        None => 80,
        Some(p) => p
            .parse()
            .map_err(|_| refuse("the port is not a number from 1 to 65535"))?,
    };
    if port == 0 {
        return Err(refuse("the port is not a number from 1 to 65535"));
    }
    let ip: IpAddr = match host.to_ascii_lowercase().as_str() {
        "localhost" | "127.0.0.1" => IpAddr::V4(Ipv4Addr::LOCALHOST),
        "[::1]" => IpAddr::V6(Ipv6Addr::LOCALHOST),
        _ => {
            return Err(refuse(
                "the host must be exactly localhost, 127.0.0.1 or [::1]",
            ))
        }
    };
    Ok(LocalEndpoint {
        addr: SocketAddr::new(ip, port),
        host_header: format!("{host}:{port}"),
    })
}

/// The local provider.
#[derive(Debug)]
pub struct LocalModel {
    endpoint: LocalEndpoint,
    settings: LocalSettings,
}

impl LocalModel {
    /// Check the URL, then check the server answers (`GET /v1/models`). A server
    /// that is not running is an error here, not a run full of failed tasks.
    pub fn connect(settings: &LocalSettings) -> Result<LocalModel> {
        let m = LocalModel::new(settings)?;
        let (status, _) = m
            .http("GET", "/v1/models", None, Duration::from_secs(10))
            .map_err(|e| {
                Error::Input(format!(
                    "no local model server answered at {}: {e}. Start one first, for example `llama-server -m model.gguf --host 127.0.0.1 --port 8080`",
                    settings.url
                ))
            })?;
        if status != 200 {
            return Err(Error::Input(format!(
                "the server at {} answered HTTP {status} to GET /v1/models, so it does not look like an OpenAI compatible server",
                settings.url
            )));
        }
        Ok(m)
    }

    /// Check the settings and the URL without contacting the server.
    pub fn new(settings: &LocalSettings) -> Result<LocalModel> {
        if !(0.0..=2.0).contains(&settings.temperature) {
            return Err(Error::Input(
                "--local-temperature must be from 0 to 2".into(),
            ));
        }
        if settings.max_new_tokens == 0 || settings.max_new_tokens > 16_384 {
            return Err(Error::Input(
                "--local-max-new-tokens must be from 1 to 16384".into(),
            ));
        }
        if settings.timeout_secs == 0 {
            return Err(Error::Input(
                "--local-timeout must be at least 1 second".into(),
            ));
        }
        Ok(LocalModel {
            endpoint: parse_local_url(&settings.url)?,
            settings: settings.clone(),
        })
    }

    /// One HTTP/1.1 request to the checked loopback address. Returns the status
    /// and the body. Never follows a redirect.
    fn http(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        timeout: Duration,
    ) -> std::result::Result<(u16, String), String> {
        let deadline = Instant::now() + timeout;
        let mut stream =
            TcpStream::connect_timeout(&self.endpoint.addr, timeout.min(Duration::from_secs(10)))
                .map_err(|e| format!("could not connect to {}: {e}", self.endpoint.addr))?;
        let peer = stream.peer_addr().map_err(|e| e.to_string())?;
        if !peer.ip().is_loopback() {
            return Err(format!("refusing to talk to non-loopback peer {peer}"));
        }
        let _ = stream.set_nodelay(true);
        let payload = body.unwrap_or("");
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nConnection: close\r\n",
            self.endpoint.host_header
        );
        if body.is_some() {
            req.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                payload.len()
            ));
        }
        req.push_str("\r\n");
        req.push_str(payload);
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| e.to_string())?;
        stream
            .write_all(req.as_bytes())
            .map_err(|e| format!("write failed: {e}"))?;

        let mut raw: Vec<u8> = Vec::new();
        let mut buf = [0u8; 16 * 1024];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("timed out after {} seconds", timeout.as_secs()));
            }
            stream
                .set_read_timeout(Some(left))
                .map_err(|e| e.to_string())?;
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    raw.extend_from_slice(&buf[..n]);
                    if raw.len() > MAX_RESPONSE_BYTES {
                        return Err("the response is larger than 4 MiB".into());
                    }
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    return Err(format!("timed out after {} seconds", timeout.as_secs()));
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("read failed: {e}")),
            }
        }
        parse_http_response(&raw)
    }

    fn chat(
        &self,
        system: &str,
        user: &str,
        seed: u64,
        max_new: u64,
    ) -> Result<(String, Option<u64>)> {
        let body = json!({
            "model": "local",
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "max_tokens": max_new,
            "temperature": self.settings.temperature,
            "seed": seed % 2_147_483_647,
            "stream": false,
        })
        .to_string();
        let (status, text) = self
            .http(
                "POST",
                "/v1/chat/completions",
                Some(&body),
                Duration::from_secs(self.settings.timeout_secs),
            )
            .map_err(|e| Error::Input(format!("local model call failed: {e}")))?;
        if status != 200 {
            let snippet: String = text.chars().take(200).collect();
            return Err(Error::Input(format!(
                "local model call failed: HTTP {status}: {snippet}"
            )));
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| {
            Error::Input(format!("local model call failed: reply is not JSON: {e}"))
        })?;
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| {
                Error::Input(
                    "local model call failed: reply has no choices[0].message.content".into(),
                )
            })?
            .to_string();
        let used = v["usage"]["total_tokens"].as_u64();
        if let Some(path) = &self.settings.transcript {
            self.append_transcript(path, seed, user, &content, used)?;
        }
        Ok((content, used))
    }

    /// Append one raw reply to the transcript file.
    fn append_transcript(
        &self,
        path: &std::path::Path,
        seed: u64,
        user: &str,
        reply: &str,
        used: Option<u64>,
    ) -> Result<()> {
        use std::io::Write;
        let tail: String = {
            let chars: Vec<char> = user.chars().collect();
            let start = chars.len().saturating_sub(300);
            chars[start..].iter().collect()
        };
        let line = json!({
            "seed": seed,
            "prompt_tail": tail,
            "reply": reply,
            "tokens": used,
        });
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(crate::error::io_err(path))?;
        writeln!(f, "{line}").map_err(crate::error::io_err(path))?;
        Ok(())
    }

    /// Cautious token guess: about 3 characters per token for the prompt, plus
    /// the most the model may write.
    fn estimate(&self, chars: usize) -> u64 {
        (chars / 3) as u64 + self.settings.max_new_tokens
    }
}

/// Split a raw HTTP/1.1 response into status and body.
fn parse_http_response(raw: &[u8]) -> std::result::Result<(u16, String), String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("the server closed the connection before a full reply")?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let body = &raw[split + 4..];
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("bad HTTP status line: {status_line:?}"))?;
    let mut chunked = false;
    let mut length: Option<usize> = None;
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim();
            if k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked") {
                chunked = true;
            }
            if k == "content-length" {
                length = v.parse().ok();
            }
        }
    }
    let bytes: Vec<u8> = if chunked {
        decode_chunked(body)?
    } else if let Some(n) = length {
        if body.len() < n {
            return Err("the reply was cut off before its full length".into());
        }
        body[..n].to_vec()
    } else {
        body.to_vec()
    };
    Ok((status, String::from_utf8_lossy(&bytes).to_string()))
}

fn decode_chunked(mut body: &[u8]) -> std::result::Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let eol = body
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("bad chunked reply")?;
        let size_text = String::from_utf8_lossy(&body[..eol]).to_string();
        let size_text = size_text.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| "bad chunk size")?;
        body = &body[eol + 2..];
        if size == 0 {
            return Ok(out);
        }
        if body.len() < size + 2 {
            return Err("the chunked reply was cut off".into());
        }
        out.extend_from_slice(&body[..size]);
        body = &body[size + 2..];
    }
}

/// Text files in the working folder the model may read, as a prompt section.
/// Skips hidden folders, build output, big files and the rules file itself.
pub fn snapshot_files(workdir: &Path, rules: &str) -> String {
    let mut files: Vec<(String, String)> = Vec::new();
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>, rules: &str) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            let Ok(ft) = e.file_type() else { continue };
            let path = e.path();
            if ft.is_dir() {
                if matches!(
                    name.as_str(),
                    ".git"
                        | ".rules-evolve"
                        | "__pycache__"
                        | "node_modules"
                        | "target"
                        | ".cursor"
                ) {
                    continue;
                }
                walk(root, &path, out, rules);
            } else if ft.is_file() {
                let Ok(meta) = e.metadata() else { continue };
                if meta.len() as usize > MAX_SNAPSHOT_FILE_BYTES {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                if text == rules || name.ends_with(".pyc") {
                    continue;
                }
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                out.push((rel, text));
            }
        }
    }
    walk(workdir, workdir, &mut files, rules);
    let mut out = String::new();
    let mut used = 0usize;
    for (rel, text) in files {
        if used + text.len() > MAX_SNAPSHOT_BYTES {
            out.push_str(&format!("(file {rel} not shown: too much to show)\n"));
            continue;
        }
        used += text.len();
        out.push_str(&format!("FILE: {rel}\n```\n{text}"));
        if !text.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("```\n\n");
    }
    if out.is_empty() {
        out.push_str("(the repository has no readable files yet)\n");
    }
    out
}

const TASK_SYSTEM: &str = "You are a coding agent. You are given a small repository and one task. \
Follow the project rules. You cannot run anything. Reply only with the files you create or change.\n\
\n\
Reply format. For each file, write one line `FILE: relative/path` and then a fenced code block that holds the COMPLETE new content of that file. \
Do not use diffs or partial files. Do not write any other files. Keep any explanation to one short sentence or leave it out.";

/// The system and user messages for a task call.
pub fn task_messages(req: &ModelRequest) -> (String, String) {
    let system = format!("{TASK_SYSTEM}\n\n## Project rules\n{}\n", req.rules);
    let user = format!(
        "## Repository files\n{}## Task\n{}\n",
        snapshot_files(req.workdir, req.rules),
        req.prompt
    );
    (system, user)
}

pub(crate) const MUTATE_SYSTEM: &str = "You improve the agent rules file of a software project. \
Reply in exactly this format and nothing else:\n\
HYPOTHESIS: <one or two plain sentences saying why the change should help>\n\
RULES:\n\
<the complete new rules text>";

fn strip_markers(line: &str) -> &str {
    line.trim()
        .trim_start_matches(['#', '*', '`', '>', '-', ' '])
        .trim_end_matches(['*', '`', ' '])
}

/// Read `FILE: path` plus fenced block pairs out of a reply.
pub fn parse_edits(reply: &str) -> Vec<FileEdit> {
    let lines: Vec<&str> = reply.lines().collect();
    let mut edits: Vec<FileEdit> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let stripped = strip_markers(lines[i]);
        let lower = stripped.to_ascii_lowercase();
        let path = if lower.starts_with("file:") {
            Some(
                stripped[5..]
                    .trim()
                    .trim_matches(['`', '*', '"', '\'', ' '])
                    .to_string(),
            )
        } else {
            None
        };
        let Some(path) = path.filter(|p| !p.is_empty()) else {
            i += 1;
            continue;
        };
        // the fence opens on one of the next few lines
        let mut j = i + 1;
        while j < lines.len() && j <= i + 3 && !lines[j].trim_start().starts_with("```") {
            if !lines[j].trim().is_empty() {
                break;
            }
            j += 1;
        }
        if j < lines.len() && lines[j].trim_start().starts_with("```") {
            let mut k = j + 1;
            let mut body: Vec<&str> = Vec::new();
            while k < lines.len() && lines[k].trim() != "```" {
                body.push(lines[k]);
                k += 1;
            }
            let mut content = body.join("\n");
            content.push('\n');
            if !edits.iter().any(|e| e.path == path) {
                edits.push(FileEdit { path, content });
            }
            i = k + 1;
        } else {
            i += 1;
        }
    }
    edits
}

/// Read `HYPOTHESIS:` and `RULES:` out of a mutation reply.
pub fn parse_mutation(reply: &str) -> std::result::Result<(String, String), String> {
    let lines: Vec<&str> = reply.lines().collect();
    let find = |label: &str| {
        lines.iter().position(|l| {
            strip_markers(l)
                .to_ascii_lowercase()
                .starts_with(&label.to_ascii_lowercase())
        })
    };
    let h = find("hypothesis:").ok_or("the model reply had no HYPOTHESIS: line")?;
    let r = find("rules:").ok_or("the model reply had no RULES: line")?;
    if r < h {
        return Err("the model reply put RULES: before HYPOTHESIS:".into());
    }
    let first = strip_markers(lines[h]);
    let mut hyp: Vec<String> = vec![first["hypothesis:".len()..]
        .trim_matches(['*', ' '])
        .to_string()];
    for l in &lines[h + 1..r] {
        hyp.push(l.trim().to_string());
    }
    let hypothesis = hyp
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if hypothesis.is_empty() {
        return Err("the model reply had an empty hypothesis".into());
    }
    let mut rest: Vec<String> = Vec::new();
    // text on the RULES: line itself
    let after_label = strip_markers(lines[r])["rules:".len()..]
        .trim_matches(['*', ' '])
        .to_string();
    if !after_label.is_empty() {
        rest.push(after_label);
    }
    rest.extend(lines[r + 1..].iter().map(|l| l.to_string()));
    // drop a surrounding fence
    while rest.first().map(|l| l.trim().is_empty()).unwrap_or(false) {
        rest.remove(0);
    }
    if rest
        .first()
        .map(|l| l.trim_start().starts_with("```"))
        .unwrap_or(false)
    {
        rest.remove(0);
        if let Some(end) = rest.iter().rposition(|l| l.trim() == "```") {
            rest.truncate(end);
        }
    }
    let mut rules = rest.join("\n").trim_end().to_string();
    if rules.trim().is_empty() {
        return Err("the model reply had empty rules text".into());
    }
    rules.push('\n');
    Ok((hypothesis, rules))
}

impl Model for LocalModel {
    fn name(&self) -> String {
        "local".to_string()
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::LocalModel
    }

    fn estimate_tokens(&self, req: &ModelRequest) -> u64 {
        let (system, user) = task_messages(req);
        self.estimate(system.len() + user.len())
    }

    fn complete(&self, req: &ModelRequest) -> Result<ModelResponse> {
        let (system, user) = task_messages(req);
        let guess = self.estimate(system.len() + user.len());
        let (reply, used) = self.chat(&system, &user, req.seed, self.settings.max_new_tokens)?;
        Ok(ModelResponse {
            edits: parse_edits(&reply),
            tokens_used: used.unwrap_or(guess),
        })
    }

    fn estimate_mutation_tokens(&self, req: &MutationRequest) -> u64 {
        self.estimate(req.prompt.len() + MUTATE_SYSTEM.len())
    }

    fn propose_mutation(&self, req: &MutationRequest) -> Result<MutationResponse> {
        let guess = self.estimate(req.prompt.len() + MUTATE_SYSTEM.len());
        let (reply, used) = self.chat(
            MUTATE_SYSTEM,
            req.prompt,
            req.seed,
            self.settings.max_new_tokens,
        )?;
        let tokens_used = used.unwrap_or(guess);
        match parse_mutation(&reply) {
            Ok((hypothesis, new_rules)) => Ok(MutationResponse {
                new_rules,
                hypothesis,
                tokens_used,
            }),
            Err(why) => Err(Error::BadReply(format!(
                "{why} (the attempt was thrown away)"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;

    /// What the fake server does with one connection.
    #[derive(Clone)]
    enum Reply {
        /// status, body, send as chunks
        Json(u16, String, bool),
        /// accept and never answer
        Hang,
        /// accept, then close with no bytes
        Close,
    }

    struct Fake {
        port: u16,
        requests: Arc<Mutex<Vec<String>>>,
        _t: JoinHandle<()>,
    }

    fn fake_server(replies: Vec<Reply>) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let t = std::thread::spawn(move || {
            let mut replies = replies.into_iter();
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { break };
                let reply = replies.next().unwrap_or(Reply::Close);
                // read headers, then the body by Content-Length
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                let mut want: Option<usize> = None;
                loop {
                    let n = s.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(&buf[..n]);
                    if let Some(p) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&data[..p]).to_ascii_lowercase();
                        let len = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        want = Some(p + 4 + len);
                    }
                    if let Some(w) = want {
                        if data.len() >= w {
                            break;
                        }
                    }
                }
                seen.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&data).to_string());
                match reply {
                    Reply::Json(status, body, chunked) => {
                        let resp = if chunked {
                            let (a, b) = body.split_at(body.len() / 2);
                            format!(
                                "HTTP/1.1 {status} X\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
                                a.len(),
                                b.len()
                            )
                        } else {
                            format!(
                                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            )
                        };
                        let _ = s.write_all(resp.as_bytes());
                    }
                    Reply::Hang => std::thread::sleep(Duration::from_secs(5)),
                    Reply::Close => {}
                }
            }
        });
        Fake {
            port,
            requests,
            _t: t,
        }
    }

    fn chat_body(content: &str, tokens: u64) -> String {
        json!({"choices":[{"message":{"role":"assistant","content":content}}],"usage":{"total_tokens":tokens}})
            .to_string()
    }

    fn settings(port: u16) -> LocalSettings {
        LocalSettings {
            url: format!("http://127.0.0.1:{port}"),
            timeout_secs: 3,
            ..Default::default()
        }
    }

    fn req<'a>(dir: &'a Path, prompt: &'a str, rules: &'a str) -> ModelRequest<'a> {
        ModelRequest {
            model: "m",
            task_id: "t1",
            prompt,
            rules,
            rules_hash: "h",
            workdir: dir,
            seed: 42,
            run_index: 0,
            max_tokens: 100_000,
        }
    }

    #[test]
    fn url_policy_accepts_only_loopback_http() {
        let ok = |u: &str| parse_local_url(u).unwrap_or_else(|e| panic!("{u}: {e}"));
        assert_eq!(
            ok("http://127.0.0.1:8080").addr.to_string(),
            "127.0.0.1:8080"
        );
        assert_eq!(ok("http://localhost:9").addr.to_string(), "127.0.0.1:9");
        assert_eq!(ok("http://LOCALHOST").addr.port(), 80);
        assert_eq!(ok("http://[::1]:8081/").addr.to_string(), "[::1]:8081");
        for bad in [
            "https://127.0.0.1:8080",
            "ftp://127.0.0.1",
            "127.0.0.1:8080",
            "http://",
            "http://api.openai.com",
            "http://api.openai.com:443",
            "http://example.com@127.0.0.1:8080",
            "http://127.0.0.1@example.com",
            "http://127.0.0.1.example.com:8080",
            "http://localhost.example.com",
            "http://127.0.0.2:8080",
            "http://0.0.0.0:8080",
            "http://[::]:8080",
            "http://192.168.1.5:8080",
            "http://10.0.0.1",
            "http://127.0.0.1:0",
            "http://127.0.0.1:99999",
            "http://127.0.0.1:abc",
            "http://127.0.0.1:8080/v1",
            "http://127.0.0.1:8080?x=1",
            "http://127.0.0.1:8080#x",
            "http://[::1",
            "http://127.0.0.1\\@evil.com",
            "",
        ] {
            let e = parse_local_url(bad).expect_err(bad);
            assert!(
                e.to_string().contains("only talks to this machine"),
                "{bad}: {e}"
            );
        }
    }

    #[test]
    fn settings_are_checked() {
        let with = |f: &dyn Fn(&mut LocalSettings)| {
            let mut s = LocalSettings::default();
            f(&mut s);
            LocalModel::new(&s)
        };
        assert!(with(&|s| s.temperature = 3.0).is_err());
        assert!(with(&|s| s.max_new_tokens = 0).is_err());
        assert!(with(&|s| s.timeout_secs = 0).is_err());
        assert!(with(&|s| s.url = "http://example.com".into()).is_err());
        assert!(LocalModel::new(&LocalSettings::default()).is_ok());
    }

    #[test]
    fn kind_is_local_model_and_needs_isolation() {
        let m = LocalModel::new(&LocalSettings::default()).unwrap();
        assert_eq!(m.kind(), ProviderKind::LocalModel);
        assert_eq!(m.name(), "local");
        assert!(m.kind().needs_isolation());
        assert!(
            crate::isolation::check_policy(m.kind(), crate::isolation::Isolation::None).is_err()
        );
    }

    #[test]
    fn connect_fails_clearly_when_no_server_runs() {
        // grab a free port, then close it
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let err = LocalModel::connect(&settings(port)).expect_err("no server");
        assert!(
            err.to_string().contains("no local model server answered"),
            "{err}"
        );
    }

    #[test]
    fn connect_checks_the_models_endpoint() {
        let f = fake_server(vec![Reply::Json(404, "nope".into(), false)]);
        let err = LocalModel::connect(&settings(f.port)).expect_err("404");
        assert!(err.to_string().contains("HTTP 404"), "{err}");
        let f = fake_server(vec![Reply::Json(200, "{\"data\":[]}".into(), false)]);
        assert!(LocalModel::connect(&settings(f.port)).is_ok());
        assert!(f.requests.lock().unwrap()[0].starts_with("GET /v1/models HTTP/1.1"));
    }

    #[test]
    fn task_call_sends_rules_task_files_and_seed_and_parses_edits() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.py"), "x = 1\n").unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "RULES TEXT").unwrap();
        std::fs::create_dir(dir.path().join(".rules-evolve")).unwrap();
        std::fs::write(dir.path().join(".rules-evolve").join("secret"), "nope").unwrap();
        let reply = "Sure.\nFILE: a.py\n```python\nx = 2\n```\n";
        let f = fake_server(vec![
            Reply::Json(200, "{}".into(), false),
            Reply::Json(200, chat_body(reply, 321), false),
        ]);
        let m = LocalModel::connect(&settings(f.port)).unwrap();
        let r = req(dir.path(), "make x two", "RULES TEXT");
        let resp = m.complete(&r).unwrap();
        assert_eq!(
            resp.edits,
            vec![FileEdit {
                path: "a.py".into(),
                content: "x = 2\n".into()
            }]
        );
        assert_eq!(resp.tokens_used, 321);
        let sent = f.requests.lock().unwrap()[1].clone();
        assert!(sent.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(sent.contains("Host: 127.0.0.1:"));
        let body: Value = serde_json::from_str(sent.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["seed"], 42);
        assert_eq!(body["stream"], false);
        assert_eq!(body["max_tokens"], 1024);
        let system = body["messages"][0]["content"].as_str().unwrap();
        let user = body["messages"][1]["content"].as_str().unwrap();
        assert!(system.contains("RULES TEXT"));
        assert!(user.contains("make x two"));
        assert!(user.contains("FILE: a.py"));
        assert!(user.contains("x = 1"));
        // the rules file copy and the tool's own folder are not shown as repo files
        assert!(!user.contains("RULES TEXT"));
        assert!(!user.contains("secret"));
    }

    #[test]
    fn transcript_gets_raw_replies() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("t.jsonl");
        let reply = "no files here, sorry";
        let f = fake_server(vec![
            Reply::Json(200, "{}".into(), false),
            Reply::Json(200, chat_body(reply, 7), false),
            Reply::Json(200, chat_body(reply, 8), false),
        ]);
        let mut s = settings(f.port);
        s.transcript = Some(log.clone());
        let m = LocalModel::connect(&s).unwrap();
        let work = tempfile::tempdir().unwrap();
        let r = req(work.path(), "do it", "R");
        assert!(m.complete(&r).unwrap().edits.is_empty());
        assert!(m.complete(&r).unwrap().edits.is_empty());
        let text = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["reply"], reply);
        assert_eq!(lines[1]["tokens"], 8);
        assert!(lines[0]["prompt_tail"].as_str().unwrap().contains("do it"));
    }

    #[test]
    fn chunked_replies_and_missing_usage_are_handled() {
        let f = fake_server(vec![
            Reply::Json(200, "{}".into(), false),
            Reply::Json(
                200,
                json!({"choices":[{"message":{"content":"FILE: b.txt\n```\nhi\n```"}}]})
                    .to_string(),
                true,
            ),
        ]);
        let m = LocalModel::connect(&settings(f.port)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let r = req(dir.path(), "p", "r");
        let resp = m.complete(&r).unwrap();
        assert_eq!(resp.edits[0].path, "b.txt");
        assert!(resp.tokens_used > 0, "falls back to an estimate");
    }

    #[test]
    fn errors_are_errors_not_empty_answers() {
        let dir = tempfile::tempdir().unwrap();
        let r = req(dir.path(), "p", "r");
        for reply in [
            Reply::Json(500, "boom".into(), false),
            Reply::Json(200, "not json".into(), false),
            Reply::Json(200, "{\"choices\":[]}".into(), false),
            Reply::Close,
        ] {
            let f = fake_server(vec![Reply::Json(200, "{}".into(), false), reply]);
            let m = LocalModel::connect(&settings(f.port)).unwrap();
            let e = m.complete(&r).expect_err("error");
            assert!(e.to_string().contains("local model call failed"), "{e}");
        }
    }

    #[test]
    fn a_hanging_server_times_out() {
        let f = fake_server(vec![Reply::Json(200, "{}".into(), false), Reply::Hang]);
        let mut s = settings(f.port);
        s.timeout_secs = 1;
        let m = LocalModel::connect(&s).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let e = m.complete(&req(dir.path(), "p", "r")).expect_err("timeout");
        assert!(e.to_string().contains("timed out"), "{e}");
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn a_reply_without_files_is_an_empty_answer() {
        let f = fake_server(vec![
            Reply::Json(200, "{}".into(), false),
            Reply::Json(200, chat_body("I would change the code.", 10), false),
        ]);
        let m = LocalModel::connect(&settings(f.port)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let resp = m.complete(&req(dir.path(), "p", "r")).unwrap();
        assert!(resp.edits.is_empty());
    }

    #[test]
    fn estimate_counts_prompt_and_max_new_tokens() {
        let m = LocalModel::new(&LocalSettings::default()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let small = m.estimate_tokens(&req(dir.path(), "p", "r"));
        assert!(small >= 1024);
        let big_rules = "x".repeat(30_000);
        let big = m.estimate_tokens(&req(dir.path(), "p", &big_rules));
        assert!(big > small + 9_000);
    }

    #[test]
    fn parse_edits_forms() {
        let e = parse_edits("**FILE: `src/a.py`**\n\n```python\nprint(1)\n```\ntext\n### File: b.md\n```\nhello\nworld\n```\n");
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].path, "src/a.py");
        assert_eq!(e[0].content, "print(1)\n");
        assert_eq!(e[1].path, "b.md");
        assert_eq!(e[1].content, "hello\nworld\n");
        // same path twice: the first wins
        let e = parse_edits("FILE: a\n```\n1\n```\nFILE: a\n```\n2\n```\n");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].content, "1\n");
        // marker without a fence, or a fence without a marker: nothing
        assert!(parse_edits("FILE: a\nno fence here").is_empty());
        assert!(parse_edits("```python\nx\n```").is_empty());
        assert!(parse_edits("FILE:\n```\nx\n```").is_empty());
        // unclosed fence still yields what was written
        assert_eq!(parse_edits("FILE: a\n```\nx").len(), 1);
    }

    #[test]
    fn parse_mutation_forms() {
        let (h, r) =
            parse_mutation("HYPOTHESIS: Run tests first.\nRULES:\n# Rules\nBe brief.\n").unwrap();
        assert_eq!(h, "Run tests first.");
        assert_eq!(r, "# Rules\nBe brief.\n");
        let (h, r) =
            parse_mutation("**HYPOTHESIS:** One.\ntwo\n**RULES:**\n```markdown\n# R\nx\n```\n")
                .unwrap();
        assert_eq!(h, "One. two");
        assert_eq!(r, "# R\nx\n");
        assert!(parse_mutation("RULES:\nx")
            .unwrap_err()
            .contains("HYPOTHESIS"));
        assert!(parse_mutation("HYPOTHESIS: h")
            .unwrap_err()
            .contains("RULES"));
        assert!(parse_mutation("RULES:\nx\nHYPOTHESIS: h").is_err());
        assert!(parse_mutation("HYPOTHESIS:\nRULES:\nx").is_err());
        assert!(parse_mutation("HYPOTHESIS: h\nRULES:\n\n").is_err());
    }

    #[test]
    fn mutation_call_and_bad_reply() {
        let f = fake_server(vec![
            Reply::Json(200, "{}".into(), false),
            Reply::Json(
                200,
                chat_body("HYPOTHESIS: h\nRULES:\nnew rules", 50),
                false,
            ),
            Reply::Json(200, chat_body("no format at all", 20), false),
        ]);
        let m = LocalModel::connect(&settings(f.port)).unwrap();
        let mreq = MutationRequest {
            model: "m",
            prompt: "improve",
            parent_rules: "old",
            parent_hash: "abc",
            seed: 7,
            max_change_bytes: 1000,
            max_tokens: 100_000,
        };
        let ok = m.propose_mutation(&mreq).unwrap();
        assert_eq!(ok.hypothesis, "h");
        assert_eq!(ok.new_rules, "new rules\n");
        assert_eq!(ok.tokens_used, 50);
        let bad = m.propose_mutation(&mreq).expect_err("bad reply");
        assert!(matches!(bad, Error::BadReply(_)), "{bad}");
        assert!(m.estimate_mutation_tokens(&mreq) >= 1024);
    }

    #[test]
    fn snapshot_skips_big_files_and_caps_total() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("big.txt"),
            "x".repeat(MAX_SNAPSHOT_FILE_BYTES + 1),
        )
        .unwrap();
        std::fs::write(dir.path().join("a.txt"), "small").unwrap();
        std::fs::write(dir.path().join("bin.dat"), [0xff, 0xfe, 0x00]).unwrap();
        let s = snapshot_files(dir.path(), "");
        assert!(s.contains("FILE: a.txt"));
        assert!(!s.contains("big.txt"));
        assert!(!s.contains("bin.dat"));
        for i in 0..6 {
            std::fs::write(
                dir.path().join(format!("f{i}.txt")),
                "y".repeat(MAX_SNAPSHOT_FILE_BYTES - 10),
            )
            .unwrap();
        }
        let s = snapshot_files(dir.path(), "");
        assert!(s.contains("not shown: too much to show"));
        assert!(s.len() < MAX_SNAPSHOT_BYTES + 4096);
    }

    #[test]
    fn build_provider_local_refuses_non_local_url_and_pace() {
        use crate::model::{build_provider, ProviderSpec};
        let spec = |url: &str, pace: u64| ProviderSpec {
            name: "local".into(),
            mock_config: "/nonexistent".into(),
            replay_file: None,
            record: None,
            pace_ms: pace,
            local: LocalSettings {
                url: url.into(),
                ..Default::default()
            },
            remote: Default::default(),
        };
        let e = build_provider(&spec("https://api.openai.com", 0))
            .err()
            .unwrap();
        assert!(e.to_string().contains("only talks to this machine"));
        let e = build_provider(&spec("http://127.0.0.1:8080", 5))
            .err()
            .unwrap();
        assert!(e.to_string().contains("--pace"));
        // unknown names still fail, and the message names the three providers
        let mut s = spec("http://127.0.0.1:8080", 0);
        s.name = "gemini".into();
        let e = build_provider(&s).err().unwrap();
        assert!(e.to_string().contains("need --allow-spend"));
    }

    #[test]
    fn recording_wraps_the_local_provider() {
        use crate::model::{build_provider, ProviderSpec};
        let f = fake_server(vec![
            Reply::Json(200, "{}".into(), false),
            Reply::Json(200, chat_body("FILE: a\n```\nx\n```", 12), false),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let rec = dir.path().join("r.jsonl");
        let model = build_provider(&ProviderSpec {
            name: "local".into(),
            mock_config: "/nonexistent".into(),
            replay_file: None,
            record: Some(rec.clone()),
            pace_ms: 0,
            local: settings(f.port),
            remote: Default::default(),
        })
        .unwrap();
        assert_eq!(model.kind(), ProviderKind::LocalModel);
        model.complete(&req(dir.path(), "p", "r")).unwrap();
        let text = std::fs::read_to_string(&rec).unwrap();
        assert!(text.contains("\"provider\":\"local\""));
        assert_eq!(model.counters().recorded, 1);
    }
}
