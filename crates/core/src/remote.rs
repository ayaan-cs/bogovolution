//! Bring your own key providers: `anthropic` (the Messages API) and `openai`
//! (any OpenAI compatible chat completions endpoint).
//!
//! **These cost money on your own key.** Every task attempt and every mutation
//! is one paid call. They are off unless all of these hold:
//!
//! - `--allow-spend` is passed on the command line (no config setting turns it
//!   on, so a copied config cannot start billing);
//! - `--api-model` names the model (there is no default model);
//! - the key is in an environment variable (`ANTHROPIC_API_KEY`,
//!   `OPENAI_API_KEY`, or the name given with `--api-key-env`).
//!
//! The key is read from the environment once, held in memory, sent only in the
//! request header, and never written anywhere: not to the log, not to replay
//! files, not to the transcript, not into error messages, and its `Debug` form
//! is redacted.
//!
//! The URL must be `https://`. Plain `http://` is allowed only for this
//! machine (localhost, 127.0.0.1, [::1]), so a key never crosses a network in
//! the clear. That is also how the tests run: against a fake server on
//! 127.0.0.1, never a real endpoint. Redirects are never followed and proxy
//! settings from the environment are ignored.
//!
//! Like `local`, these providers only return text and run no tools. Their kind
//! is [`ProviderKind::Real`], so a run is refused unless an isolation backend
//! is in use.

use crate::error::{Error, Result};
use crate::isolation::ProviderKind;
use crate::local::{parse_edits, parse_mutation, task_messages, MUTATE_SYSTEM};
use crate::model::{Model, ModelRequest, ModelResponse, MutationRequest, MutationResponse};
use serde_json::{json, Value};
use std::io::Read;
use std::time::Duration;

/// Largest response body read from the API.
const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
/// Anthropic API version header value.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Which wire format to speak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    /// `POST {base}/v1/messages`, `x-api-key` header.
    Anthropic,
    /// `POST {base}/chat/completions`, `Authorization: Bearer` header. The base
    /// URL includes the version path, for example `https://api.openai.com/v1`.
    OpenAi,
}

impl Api {
    /// Parse a provider name. `None` for names that are not remote providers.
    pub fn from_provider(name: &str) -> Option<Api> {
        match name {
            "anthropic" => Some(Api::Anthropic),
            "openai" => Some(Api::OpenAi),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Api::Anthropic => "anthropic",
            Api::OpenAi => "openai",
        }
    }

    pub fn default_url(self) -> &'static str {
        match self {
            Api::Anthropic => "https://api.anthropic.com",
            Api::OpenAi => "https://api.openai.com/v1",
        }
    }

    pub fn default_key_env(self) -> &'static str {
        match self {
            Api::Anthropic => "ANTHROPIC_API_KEY",
            Api::OpenAi => "OPENAI_API_KEY",
        }
    }

    fn path(self) -> &'static str {
        match self {
            Api::Anthropic => "/v1/messages",
            Api::OpenAi => "/chat/completions",
        }
    }
}

/// Settings for a remote provider (command line flags). Holds the NAME of the
/// key variable, never the key.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RemoteSettings {
    /// Base URL. Empty means the provider's default.
    pub url: String,
    /// Model id sent to the API. Required.
    pub model: String,
    /// Environment variable holding the key. Empty means the default.
    pub key_env: String,
    /// Must be true, from `--allow-spend`.
    pub allow_spend: bool,
    pub temperature: f64,
    /// Send no `temperature` at all (from `--api-no-temperature`). Newer
    /// Anthropic models refuse the field ("`temperature` is deprecated for
    /// this model").
    pub omit_temperature: bool,
    pub max_new_tokens: u64,
    /// Anthropic only: `adaptive`, `off` (sent as `thinking: {type:
    /// "disabled"}`) or `between_tools` (Claude Sonnet 5.5 refuses
    /// `disabled` and turns up-front thinking off with this). Empty sends no `thinking` field, so the model's default
    /// applies. Thinking counts against `max_new_tokens`, so a cut off
    /// answer can be all thinking and no files (Claude run 2).
    pub thinking: String,
    /// Anthropic only: `low`, `medium`, `high`, `xhigh` or `max`, sent as
    /// `output_config.effort`. Empty sends nothing.
    pub effort: String,
    pub timeout_secs: u64,
    /// Optional JSONL file that gets every raw model reply.
    pub transcript: Option<std::path::PathBuf>,
}

impl RemoteSettings {
    /// The defaults the CLI uses.
    pub fn defaults() -> RemoteSettings {
        RemoteSettings {
            temperature: 0.7,
            max_new_tokens: 1024,
            timeout_secs: 300,
            ..Default::default()
        }
    }
}

/// The key, kept out of every printed form.
#[derive(Clone)]
struct Secret(String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Check a base URL: `https://` anywhere, or `http://` to this machine only.
/// Returns the URL without a trailing slash.
pub fn check_url(url: &str) -> Result<String> {
    let refuse = |why: &str| {
        Error::Input(format!(
            "--api-url {url:?} is not allowed: {why}. Use an https:// URL, or http:// only for localhost, 127.0.0.1 or [::1]"
        ))
    };
    let (scheme, rest) = if let Some(r) = url.strip_prefix("https://") {
        ("https", r)
    } else if let Some(r) = url.strip_prefix("http://") {
        ("http", r)
    } else {
        return Err(refuse("the scheme must be https://"));
    };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty() {
        return Err(refuse("there is no host"));
    }
    if rest.contains(['@', '?', '#', '\\', ' ']) {
        return Err(refuse(
            "it must not hold user info, a query, a fragment or spaces",
        ));
    }
    let host = if let Some(h) = authority.strip_prefix('[') {
        h.split(']').next().unwrap_or("").to_string()
    } else {
        authority
            .split(':')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    if scheme == "http" && !matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1") {
        return Err(refuse(
            "plain http would send your key in the clear, so it is only allowed to this machine",
        ));
    }
    Ok(url.trim_end_matches('/').to_string())
}

/// A paid, bring your own key provider.
#[derive(Debug)]
pub struct RemoteModel {
    api: Api,
    url: String,
    key: Secret,
    settings: RemoteSettings,
    agent: ureq::Agent,
}

impl RemoteModel {
    /// Check the settings and read the key from the environment. Makes no
    /// network call.
    pub fn new(api: Api, settings: &RemoteSettings) -> Result<RemoteModel> {
        Self::with_env(api, settings, |k| std::env::var(k).ok())
    }

    /// Like [`RemoteModel::new`] with the environment lookup passed in (tests).
    pub fn with_env(
        api: Api,
        settings: &RemoteSettings,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<RemoteModel> {
        let name = api.name();
        if !settings.allow_spend {
            return Err(Error::Input(format!(
                "--provider {name} calls a paid API on your own key, and every task attempt and mutation is a billed call. Pass --allow-spend to confirm you want that. Nothing was sent"
            )));
        }
        if settings.model.trim().is_empty() {
            return Err(Error::Input(format!(
                "--provider {name} needs --api-model <MODEL ID>. There is no default model, so you always choose what you pay for"
            )));
        }
        if !(0.0..=2.0).contains(&settings.temperature) {
            return Err(Error::Input("--api-temperature must be from 0 to 2".into()));
        }
        if settings.max_new_tokens == 0 || settings.max_new_tokens > 16_384 {
            return Err(Error::Input(
                "--api-max-new-tokens must be from 1 to 16384".into(),
            ));
        }
        if settings.timeout_secs == 0 {
            return Err(Error::Input(
                "--api-timeout must be at least 1 second".into(),
            ));
        }
        check_thinking(api, &settings.thinking, &settings.effort)?;
        let url = check_url(if settings.url.is_empty() {
            api.default_url()
        } else {
            &settings.url
        })?;
        let key_env = if settings.key_env.is_empty() {
            api.default_key_env()
        } else {
            settings.key_env.as_str()
        };
        if key_env.is_empty()
            || !key_env
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(Error::Input(
                "--api-key-env must be the NAME of an environment variable (letters, digits, _), not the key itself".into(),
            ));
        }
        let key = env(key_env).unwrap_or_default();
        let key = key.trim().to_string();
        if key.is_empty() {
            return Err(Error::Input(format!(
                "the environment variable {key_env} is not set or empty. Put your {name} key there (it is never written to disk by graft)"
            )));
        }
        if key.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(Error::Input(format!(
                "the key in {key_env} holds spaces or control characters, so it is not used"
            )));
        }
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(settings.timeout_secs))
            .redirects(0)
            .try_proxy_from_env(false)
            .user_agent(concat!("graft/", env!("CARGO_PKG_VERSION")))
            .build();
        Ok(RemoteModel {
            api,
            url,
            key: Secret(key),
            settings: settings.clone(),
            agent,
        })
    }

    /// The request body for one call.
    fn body(&self, system: &str, user: &str, seed: u64) -> Value {
        let mut body = self.body_with_temperature(system, user, seed);
        if self.settings.omit_temperature {
            if let Some(map) = body.as_object_mut() {
                map.remove("temperature");
            }
        }
        body
    }

    fn body_with_temperature(&self, system: &str, user: &str, seed: u64) -> Value {
        match self.api {
            Api::Anthropic => {
                let mut body = json!({
                    "model": self.settings.model,
                    "max_tokens": self.settings.max_new_tokens,
                    "temperature": self.settings.temperature.min(1.0),
                    "system": system,
                    "messages": [{"role": "user", "content": user}],
                });
                match self.settings.thinking.as_str() {
                    "off" => body["thinking"] = json!({"type": "disabled"}),
                    "adaptive" => body["thinking"] = json!({"type": "adaptive"}),
                    "between_tools" => body["thinking"] = json!({"type": "between_tools"}),
                    _ => {}
                }
                if !self.settings.effort.is_empty() {
                    body["output_config"] = json!({"effort": self.settings.effort});
                }
                body
            }
            Api::OpenAi => json!({
                "model": self.settings.model,
                "max_tokens": self.settings.max_new_tokens,
                "temperature": self.settings.temperature,
                "seed": seed % 2_147_483_647,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
            }),
        }
    }

    /// Strip anything that looks like the key from text before it is shown.
    fn scrub(&self, text: &str) -> String {
        text.replace(&self.key.0, "<redacted>")
    }

    fn chat(&self, system: &str, user: &str, seed: u64) -> Result<(String, Option<u64>)> {
        let name = self.api.name();
        let url = format!("{}{}", self.url, self.api.path());
        let req = self
            .agent
            .post(&url)
            .set("content-type", "application/json")
            .set("accept", "application/json");
        let req = match self.api {
            Api::Anthropic => req
                .set("x-api-key", &self.key.0)
                .set("anthropic-version", ANTHROPIC_VERSION),
            Api::OpenAi => req.set("authorization", &format!("Bearer {}", self.key.0)),
        };
        let body = self.body(system, user, seed).to_string();
        let (status, text) = match req.send_string(&body) {
            Ok(r) => (r.status(), read_body(r)),
            Err(ureq::Error::Status(code, r)) => (code, read_body(r)),
            Err(ureq::Error::Transport(t)) => {
                return Err(Error::Input(format!(
                    "{name} call failed: {}",
                    self.scrub(&t.to_string())
                )))
            }
        };
        let text = text.map_err(|e| Error::Input(format!("{name} call failed: {e}")))?;
        if status != 200 {
            let snippet: String = self.scrub(&text).chars().take(300).collect();
            return Err(Error::Input(format!(
                "{name} call failed: HTTP {status}: {snippet}"
            )));
        }
        let v: Value = serde_json::from_str(&text)
            .map_err(|e| Error::Input(format!("{name} call failed: reply is not JSON: {e}")))?;
        let usage = v["usage"].clone();
        let stop = v["stop_reason"].clone();
        let (content, used) = match self.api {
            Api::Anthropic => {
                let parts = v["content"].as_array().ok_or_else(|| {
                    Error::Input(format!("{name} call failed: reply has no content list"))
                })?;
                let text: String = parts
                    .iter()
                    .filter(|p| p["type"] == "text")
                    .filter_map(|p| p["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("");
                let used = match (
                    v["usage"]["input_tokens"].as_u64(),
                    v["usage"]["output_tokens"].as_u64(),
                ) {
                    (Some(i), Some(o)) => Some(i + o),
                    _ => None,
                };
                (text, used)
            }
            Api::OpenAi => {
                let text = v["choices"][0]["message"]["content"]
                    .as_str()
                    .ok_or_else(|| {
                        Error::Input(format!(
                            "{name} call failed: reply has no choices[0].message.content"
                        ))
                    })?
                    .to_string();
                (text, v["usage"]["total_tokens"].as_u64())
            }
        };
        if let Some(path) = &self.settings.transcript {
            self.append_transcript(path, seed, &content, used, &usage, &stop)?;
        }
        Ok((content, used))
    }

    fn append_transcript(
        &self,
        path: &std::path::Path,
        seed: u64,
        reply: &str,
        used: Option<u64>,
        usage: &Value,
        stop: &Value,
    ) -> Result<()> {
        use std::io::Write;
        let line = json!({
            "provider": self.api.name(),
            "model": self.settings.model,
            "seed": seed,
            "reply": self.scrub(reply),
            "tokens": used,
            // The provider's own counts (input and output apart), for cost.
            "usage": usage,
            // Why the reply ended: "max_tokens" means it was cut off at
            // --api-max-new-tokens (null for providers that do not say).
            "stop_reason": stop,
            "thinking": self.settings.thinking,
            "effort": self.settings.effort,
        });
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(crate::error::io_err(path))?;
        writeln!(f, "{line}").map_err(crate::error::io_err(path))?;
        Ok(())
    }

    fn estimate(&self, chars: usize) -> u64 {
        (chars / 3) as u64 + self.settings.max_new_tokens
    }
}

/// `--api-thinking` and `--api-effort`: known values, Anthropic only, and not
/// a pair the API refuses (thinking off works at effort `high` or below).
fn check_thinking(api: Api, thinking: &str, effort: &str) -> Result<()> {
    if !matches!(thinking, "" | "adaptive" | "off" | "between_tools") {
        return Err(Error::Input(format!(
            "--api-thinking must be adaptive, off or between_tools, not {thinking:?}"
        )));
    }
    if !matches!(effort, "" | "low" | "medium" | "high" | "xhigh" | "max") {
        return Err(Error::Input(format!(
            "--api-effort must be low, medium, high, xhigh or max, not {effort:?}"
        )));
    }
    if api != Api::Anthropic && !(thinking.is_empty() && effort.is_empty()) {
        return Err(Error::Input(
            "--api-thinking and --api-effort are only for --provider anthropic".into(),
        ));
    }
    if matches!(thinking, "off" | "between_tools") && matches!(effort, "xhigh" | "max") {
        return Err(Error::Input(format!(
            "--api-thinking {thinking} works only with --api-effort low, medium or high (the API refuses it at xhigh and max)"
        )));
    }
    Ok(())
}

fn read_body(r: ureq::Response) -> std::result::Result<String, String> {
    let mut buf = Vec::new();
    r.into_reader()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("read failed: {e}"))?;
    if buf.len() as u64 > MAX_RESPONSE_BYTES {
        return Err("the response is larger than 4 MiB".into());
    }
    String::from_utf8(buf).map_err(|_| "the response is not UTF-8".into())
}

impl Model for RemoteModel {
    fn name(&self) -> String {
        self.api.name().to_string()
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Real
    }

    fn estimate_tokens(&self, req: &ModelRequest) -> u64 {
        let (system, user) = task_messages(req);
        self.estimate(system.len() + user.len())
    }

    fn complete(&self, req: &ModelRequest) -> Result<ModelResponse> {
        let (system, user) = task_messages(req);
        let guess = self.estimate(system.len() + user.len());
        let (reply, used) = self.chat(&system, &user, req.seed)?;
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
        let (reply, used) = self.chat(MUTATE_SYSTEM, req.prompt, req.seed)?;
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
    //! Every test here talks to a fake server on 127.0.0.1. None reaches a real
    //! endpoint.
    use super::*;
    use crate::model::FileEdit;
    use std::io::Write;
    use std::net::TcpListener;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    const KEY: &str = "sk-test-NOT-A-REAL-KEY-0123456789";

    struct Fake {
        port: u16,
        requests: Arc<Mutex<Vec<String>>>,
    }

    /// Serve the given (status, extra headers, body) replies, one per connection.
    fn fake(replies: Vec<(u16, &'static str, String)>) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        std::thread::spawn(move || {
            for (status, extra, body) in replies {
                let Ok((mut s, _)) = listener.accept() else {
                    return;
                };
                let mut raw = Vec::new();
                let mut buf = [0u8; 8192];
                loop {
                    let n = s.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&raw).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text[..end]
                            .lines()
                            .find_map(|l| {
                                let l = l.to_ascii_lowercase();
                                l.strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if raw.len() >= end + 4 + len {
                            break;
                        }
                    }
                }
                seen.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&raw).to_string());
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(reply.as_bytes());
            }
        });
        Fake { port, requests }
    }

    fn settings(url: String) -> RemoteSettings {
        RemoteSettings {
            url,
            model: "test-model-1".into(),
            allow_spend: true,
            timeout_secs: 5,
            ..RemoteSettings::defaults()
        }
    }

    fn env_with_key(name: &str) -> Option<String> {
        matches!(name, "ANTHROPIC_API_KEY" | "OPENAI_API_KEY" | "MY_KEY").then(|| KEY.to_string())
    }

    fn model(api: Api, s: &RemoteSettings) -> RemoteModel {
        RemoteModel::with_env(api, s, env_with_key).unwrap()
    }

    fn req<'a>(dir: &'a Path) -> ModelRequest<'a> {
        ModelRequest {
            model: "m",
            task_id: "t1",
            prompt: "make a",
            rules: "Be brief.",
            rules_hash: "h",
            workdir: dir,
            seed: 42,
            run_index: 0,
            max_tokens: 100_000,
            context: &[],
        }
    }

    fn anthropic_reply(text: &str) -> String {
        json!({
            "content": [{"type": "text", "text": text}],
            "usage": {"input_tokens": 30, "output_tokens": 12}
        })
        .to_string()
    }

    fn openai_reply(text: &str) -> String {
        json!({
            "choices": [{"message": {"content": text}}],
            "usage": {"total_tokens": 55}
        })
        .to_string()
    }

    fn err_of(r: Result<RemoteModel>) -> String {
        r.expect_err("should fail").to_string()
    }

    #[test]
    fn needs_allow_spend_a_model_and_a_key_before_anything_is_sent() {
        let base = settings("http://127.0.0.1:9".into());
        let mut s = base.clone();
        s.allow_spend = false;
        let e = err_of(RemoteModel::with_env(Api::Anthropic, &s, env_with_key));
        assert!(
            e.contains("--allow-spend") && e.contains("Nothing was sent"),
            "{e}"
        );
        let mut s = base.clone();
        s.model = " ".into();
        let e = err_of(RemoteModel::with_env(Api::OpenAi, &s, env_with_key));
        assert!(e.contains("--api-model"), "{e}");
        let e = err_of(RemoteModel::with_env(Api::Anthropic, &base, |_| None));
        assert!(e.contains("ANTHROPIC_API_KEY is not set"), "{e}");
        let e = err_of(RemoteModel::with_env(Api::OpenAi, &base, |_| {
            Some("  ".into())
        }));
        assert!(e.contains("OPENAI_API_KEY"), "{e}");
        // a key pasted where the variable name goes is refused, and not echoed
        let mut s = base.clone();
        s.key_env = KEY.into();
        let e = err_of(RemoteModel::with_env(Api::OpenAi, &s, env_with_key));
        assert!(
            e.contains("NAME of an environment variable") && !e.contains(KEY),
            "{e}"
        );
        let mut s = base.clone();
        s.key_env = "MY_KEY".into();
        assert!(RemoteModel::with_env(Api::OpenAi, &s, env_with_key).is_ok());
        let e = err_of(RemoteModel::with_env(Api::OpenAi, &base, |_| {
            Some("sk bad\nkey".into())
        }));
        assert!(
            e.contains("spaces or control") && !e.contains("sk bad"),
            "{e}"
        );
        for (field, bad) in [("t", 2.5), ("n", 0.0), ("s", 0.0)] {
            let mut s = base.clone();
            match field {
                "t" => s.temperature = bad,
                "n" => s.max_new_tokens = 0,
                _ => s.timeout_secs = 0,
            }
            assert!(RemoteModel::with_env(Api::OpenAi, &s, env_with_key).is_err());
        }
    }

    #[test]
    fn url_policy_is_https_or_loopback_http() {
        for ok in [
            "https://api.anthropic.com",
            "https://api.openai.com/v1/",
            "https://openrouter.ai/api/v1",
            "http://127.0.0.1:8080/v1",
            "http://localhost:1234",
            "http://[::1]:9/v1",
        ] {
            assert!(check_url(ok).is_ok(), "{ok}");
        }
        assert_eq!(
            check_url("https://api.openai.com/v1/").unwrap(),
            "https://api.openai.com/v1"
        );
        for bad in [
            "http://api.openai.com/v1",
            "http://10.0.0.5:8080",
            "http://127.0.0.1.evil.com",
            "ftp://x",
            "https://",
            "https://user:pw@host",
            "https://host/v1?x=1",
            "api.openai.com",
        ] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn defaults_point_at_the_real_apis_but_tests_never_use_them() {
        assert_eq!(Api::Anthropic.default_url(), "https://api.anthropic.com");
        assert_eq!(Api::OpenAi.default_url(), "https://api.openai.com/v1");
        assert_eq!(Api::from_provider("anthropic"), Some(Api::Anthropic));
        assert_eq!(Api::from_provider("openai"), Some(Api::OpenAi));
        assert_eq!(Api::from_provider("local"), None);
    }

    #[test]
    fn anthropic_request_shape_and_reply() {
        let f = fake(vec![(
            200,
            "",
            anthropic_reply("FILE: a.txt\n```\nhello\n```"),
        )]);
        let m = model(
            Api::Anthropic,
            &settings(format!("http://127.0.0.1:{}", f.port)),
        );
        assert_eq!(m.kind(), ProviderKind::Real);
        assert_eq!(m.name(), "anthropic");
        let dir = tempfile::tempdir().unwrap();
        let r = m.complete(&req(dir.path())).unwrap();
        assert_eq!(
            r.edits,
            vec![FileEdit {
                path: "a.txt".into(),
                content: "hello\n".into()
            }]
        );
        assert_eq!(r.tokens_used, 42);
        let seen = f.requests.lock().unwrap()[0].clone();
        let lower = seen.to_ascii_lowercase();
        assert!(seen.starts_with("POST /v1/messages HTTP/1.1"), "{seen}");
        assert!(lower.contains(&format!("x-api-key: {}", KEY.to_ascii_lowercase())));
        assert!(lower.contains("anthropic-version: 2023-06-01"));
        assert!(!lower.contains("authorization:"));
        let body: Value =
            serde_json::from_str(&seen[seen.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body["model"], "test-model-1");
        assert_eq!(body["max_tokens"], 1024);
        assert!(body["system"].as_str().unwrap().contains("Be brief."));
        assert_eq!(body["messages"][0]["role"], "user");
        assert!(body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("make a"));
    }

    #[test]
    fn temperature_is_sent_unless_omitted_and_the_transcript_keeps_usage() {
        let dir = tempfile::tempdir().unwrap();
        let body_of = |omit: bool| {
            let f = fake(vec![(200, "", anthropic_reply("no files"))]);
            let mut s = settings(format!("http://127.0.0.1:{}", f.port));
            s.omit_temperature = omit;
            s.transcript = Some(dir.path().join("t.jsonl"));
            let m = model(Api::Anthropic, &s);
            m.complete(&req(dir.path())).unwrap();
            let seen = f.requests.lock().unwrap()[0].clone();
            serde_json::from_str::<Value>(&seen[seen.find("\r\n\r\n").unwrap() + 4..]).unwrap()
        };
        assert_eq!(body_of(false)["temperature"], 0.7);
        let b = body_of(true);
        assert!(b.get("temperature").is_none(), "{b}");
        assert_eq!(b["max_tokens"], 1024);
        let t = std::fs::read_to_string(dir.path().join("t.jsonl")).unwrap();
        let line: Value = serde_json::from_str(t.lines().next().unwrap()).unwrap();
        assert_eq!(line["usage"]["input_tokens"], 30);
        assert_eq!(line["usage"]["output_tokens"], 12);
    }

    #[test]
    fn thinking_and_effort_are_sent_only_when_set_and_the_transcript_says_why_a_reply_ended() {
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path().join("t.jsonl");
        let body_of = |thinking: &str, effort: &str, reply: String| {
            let f = fake(vec![(200, "", reply)]);
            let mut s = settings(format!("http://127.0.0.1:{}", f.port));
            s.omit_temperature = true;
            s.thinking = thinking.into();
            s.effort = effort.into();
            s.transcript = Some(t.clone());
            let m = model(Api::Anthropic, &s);
            m.complete(&req(dir.path())).unwrap();
            let seen = f.requests.lock().unwrap()[0].clone();
            serde_json::from_str::<Value>(&seen[seen.find("\r\n\r\n").unwrap() + 4..]).unwrap()
        };
        let b = body_of("", "", anthropic_reply("no files"));
        assert!(b.get("thinking").is_none(), "{b}");
        assert!(b.get("output_config").is_none(), "{b}");
        let b = body_of("off", "medium", anthropic_reply("no files"));
        assert_eq!(b["thinking"], json!({"type": "disabled"}));
        assert_eq!(b["output_config"], json!({"effort": "medium"}));
        let b = body_of("between_tools", "high", anthropic_reply("no files"));
        assert_eq!(b["thinking"], json!({"type": "between_tools"}));
        let b = body_of("adaptive", "", anthropic_reply("no files"));
        assert_eq!(b["thinking"], json!({"type": "adaptive"}));
        assert!(b.get("output_config").is_none(), "{b}");
        // A reply cut off at the cap, all thinking: no files, and the
        // transcript says max_tokens.
        let cut = json!({
            "content": [{"type": "thinking", "thinking": "hmm"}],
            "stop_reason": "max_tokens",
            "usage": {"input_tokens": 30, "output_tokens": 1024}
        })
        .to_string();
        body_of("", "", cut);
        let lines: Vec<Value> = std::fs::read_to_string(&t)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[1]["thinking"], "off");
        assert_eq!(lines[1]["effort"], "medium");
        assert_eq!(lines[2]["thinking"], "between_tools");
        assert_eq!(lines[4]["stop_reason"], "max_tokens");
        assert_eq!(lines[4]["reply"], "");
    }

    #[test]
    fn bad_thinking_settings_are_refused_before_anything_is_sent() {
        let base = settings("http://127.0.0.1:9".into());
        let with = |api: Api, thinking: &str, effort: &str| {
            let mut s = base.clone();
            s.thinking = thinking.into();
            s.effort = effort.into();
            RemoteModel::with_env(api, &s, env_with_key)
        };
        assert!(err_of(with(Api::Anthropic, "budget", "")).contains("--api-thinking"));
        assert!(err_of(with(Api::Anthropic, "", "huge")).contains("--api-effort"));
        assert!(err_of(with(Api::Anthropic, "off", "xhigh")).contains("low, medium or high"));
        assert!(err_of(with(Api::Anthropic, "off", "max")).contains("low, medium or high"));
        assert!(err_of(with(Api::OpenAi, "off", "")).contains("only for --provider anthropic"));
        assert!(with(Api::Anthropic, "off", "high").is_ok());
        assert!(with(Api::Anthropic, "between_tools", "high").is_ok());
        assert!(
            err_of(with(Api::Anthropic, "between_tools", "max")).contains("low, medium or high")
        );
        assert!(err_of(with(Api::Anthropic, "none", "")).contains("adaptive, off or between_tools"));
        assert!(with(Api::Anthropic, "adaptive", "max").is_ok());
        assert!(with(Api::OpenAi, "", "").is_ok());
    }

    #[test]
    fn openai_request_shape_and_mutation_reply() {
        let f = fake(vec![(
            200,
            "",
            openai_reply("HYPOTHESIS: shorter is better\nRULES:\nBe very brief."),
        )]);
        let m = model(
            Api::OpenAi,
            &settings(format!("http://127.0.0.1:{}/v1", f.port)),
        );
        let out = m
            .propose_mutation(&MutationRequest {
                model: "m",
                prompt: "improve it",
                parent_rules: "Be brief.",
                parent_hash: "h",
                seed: 7,
                max_change_bytes: 1000,
                max_tokens: 10_000,
            })
            .unwrap();
        assert_eq!(out.hypothesis, "shorter is better");
        assert_eq!(out.new_rules, "Be very brief.\n");
        assert_eq!(out.tokens_used, 55);
        let seen = f.requests.lock().unwrap()[0].clone();
        assert!(
            seen.starts_with("POST /v1/chat/completions HTTP/1.1"),
            "{seen}"
        );
        assert!(seen.to_ascii_lowercase().contains(&format!(
            "authorization: bearer {}",
            KEY.to_ascii_lowercase()
        )));
        let body: Value =
            serde_json::from_str(&seen[seen.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body["seed"], 7);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "improve it");
    }

    #[test]
    fn the_key_never_appears_in_errors_transcripts_replays_or_debug() {
        let dir = tempfile::tempdir().unwrap();
        // the server echoes the key back in an error and in a reply
        let f = fake(vec![
            (401, "", format!("{{\"error\":\"bad key {KEY}\"}}")),
            (200, "", openai_reply(&format!("FILE: k\n```\n{KEY}\n```"))),
            (200, "", openai_reply("FILE: b\n```\nb\n```")),
        ]);
        let transcript = dir.path().join("t.jsonl");
        let mut s = settings(format!("http://127.0.0.1:{}/v1", f.port));
        s.transcript = Some(transcript.clone());
        let m = model(Api::OpenAi, &s);
        assert!(!format!("{m:?}").contains(KEY));
        let e = m.complete(&req(dir.path())).unwrap_err().to_string();
        assert!(
            e.contains("HTTP 401") && e.contains("<redacted>") && !e.contains(KEY),
            "{e}"
        );
        m.complete(&req(dir.path())).unwrap();

        // through build_provider with recording, as the CLI does it
        let rec = dir.path().join("rec.jsonl");
        let spec = crate::model::ProviderSpec {
            name: "openai".into(),
            mock_config: "/nonexistent".into(),
            replay_file: None,
            record: Some(rec.clone()),
            pace_ms: 0,
            local: Default::default(),
            remote: RemoteSettings {
                key_env: "REVOLVE_TEST_KEY_THAT_IS_NOT_SET".into(),
                ..s.clone()
            },
        };
        let e = crate::model::build_provider(&spec)
            .err()
            .unwrap()
            .to_string();
        assert!(
            e.contains("REVOLVE_TEST_KEY_THAT_IS_NOT_SET is not set"),
            "{e}"
        );
        let inner: Box<dyn Model> = Box::new(model(Api::OpenAi, &s));
        let recorded = crate::replay::RecordingModel::new(inner, &rec).unwrap();
        recorded.complete(&req(dir.path())).unwrap();
        let text = std::fs::read_to_string(&rec).unwrap();
        assert!(text.contains("\"provider\":\"openai\""));
        assert!(!text.contains(KEY));
        let t = std::fs::read_to_string(&transcript).unwrap();
        assert!(t.contains("<redacted>") && !t.contains(KEY), "{t}");
        assert!(!f.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn redirects_are_not_followed_and_bad_replies_are_errors() {
        let f = fake(vec![
            (
                302,
                "Location: http://127.0.0.1:9/elsewhere\r\n",
                "{}".into(),
            ),
            (200, "", "not json".into()),
            (200, "", "{\"choices\":[]}".into()),
        ]);
        let m = model(
            Api::OpenAi,
            &settings(format!("http://127.0.0.1:{}/v1", f.port)),
        );
        let dir = tempfile::tempdir().unwrap();
        let e = m.complete(&req(dir.path())).unwrap_err().to_string();
        assert!(e.contains("HTTP 302"), "{e}");
        let e = m.complete(&req(dir.path())).unwrap_err().to_string();
        assert!(e.contains("not JSON"), "{e}");
        let e = m.complete(&req(dir.path())).unwrap_err().to_string();
        assert!(e.contains("no choices"), "{e}");
        assert_eq!(f.requests.lock().unwrap().len(), 3);
    }

    #[test]
    fn nothing_listening_is_a_clear_error() {
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let m = model(
            Api::Anthropic,
            &settings(format!("http://127.0.0.1:{port}")),
        );
        let dir = tempfile::tempdir().unwrap();
        let e = m.complete(&req(dir.path())).unwrap_err().to_string();
        assert!(e.starts_with("anthropic call failed"), "{e}");
        assert!(!e.contains(KEY));
    }

    #[test]
    fn a_paid_provider_cannot_run_without_isolation() {
        use crate::isolation::{check_policy, Isolation};
        assert!(check_policy(ProviderKind::Real, Isolation::None).is_err());
    }
}
