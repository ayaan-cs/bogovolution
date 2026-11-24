// Structural checks for the Claude Code plugin. Zero dependencies: node:test only.
//   node --test tests/*.test.mjs
// The rules come from the official plugin docs (plugin manifest reference, hooks
// reference, skills reference, marketplace guide). `claude plugin validate` is the
// authoritative check and runs in run-all.sh when the claude CLI is installed.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "..");
const repo = path.resolve(root, "../..");
const pluginDir = path.join(root, "plugin");
const read = (p) => fs.readFileSync(p, "utf8");
const readJson = (p) => JSON.parse(read(p));

const manifest = readJson(path.join(pluginDir, ".claude-plugin/plugin.json"));
const marketplace = readJson(path.join(root, ".claude-plugin/marketplace.json"));
const hooksFile = readJson(path.join(pluginDir, "hooks/hooks.json"));

// Top level plugin.json keys listed in the manifest reference (unknown ones are stripped).
const MANIFEST_KEYS = new Set([
  "$schema", "name", "displayName", "version", "description", "author", "homepage",
  "repository", "license", "keywords", "metadata", "icon", "documentationUrl",
  "supportUrl", "privacyPolicyUrl", "termsOfServiceUrl", "defaultEnabled",
  "dependencies", "settings", "userConfig", "types", "channels", "skills", "commands",
  "agents", "hooks", "mcpServers", "lspServers", "outputStyles", "workflows", "experimental",
]);
// Hook events named in the hooks reference that this plugin may use.
const HOOK_EVENTS = new Set(["SessionStart", "PostToolUse", "PreToolUse", "UserPromptSubmit", "Stop", "SessionEnd"]);
// Skill frontmatter fields from the skills reference (the ones this plugin uses).
const SKILL_FIELDS = new Set([
  "name", "description", "argument-hint", "disable-model-invocation", "user-invocable",
  "allowed-tools", "disallowed-tools", "when_to_use", "arguments", "model", "context", "paths",
]);
const EXPECTED_SKILLS = ["apply", "digest", "evolve", "export", "init", "propose", "status", "verify-log"];

function frontmatter(file) {
  const text = read(file);
  assert.ok(text.startsWith("---\n"), `${file}: frontmatter must start on line 1`);
  const end = text.indexOf("\n---\n", 4);
  assert.ok(end > 0, `${file}: frontmatter is not closed`);
  const fields = {};
  for (const line of text.slice(4, end).split("\n")) {
    const m = line.match(/^([A-Za-z_-]+):\s*(.*)$/);
    assert.ok(m, `${file}: cannot parse frontmatter line: ${line}`);
    let v = m[2].trim();
    if (v.startsWith('"') && v.endsWith('"')) v = v.slice(1, -1);
    fields[m[1]] = v;
  }
  return { fields, body: text.slice(end + 5) };
}

function walk(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (e.name === "node_modules" || e.name === "target") continue;
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p, out);
    else out.push(p);
  }
  return out;
}

test("plugin.json: required fields, kebab-case name, only documented keys", () => {
  assert.equal(manifest.name, "graft");
  assert.match(manifest.name, /^[a-z0-9]+(-[a-z0-9]+)*$/);
  // reserved names: claude-, anthropic-, official..., or the bare words
  assert.doesNotMatch(manifest.name, /^(claude|anthropic|anthropics|cc-plugin)(-|$)|official/i);
  assert.equal(manifest.version, "4.0.0");
  assert.ok(manifest.description.length > 20);
  assert.ok(manifest.author && typeof manifest.author.name === "string");
  for (const k of Object.keys(manifest)) assert.ok(MANIFEST_KEYS.has(k), `unknown plugin.json key: ${k}`);
  if (manifest.homepage) assert.doesNotThrow(() => new URL(manifest.homepage));
});

test("plugin.json: no component keys that would replace the default folders", () => {
  for (const k of ["commands", "agents", "outputStyles", "workflows"]) assert.equal(manifest[k], undefined, k);
});

test("plugin layout: only .claude-plugin/ holds the manifest", () => {
  assert.deepEqual(fs.readdirSync(path.join(pluginDir, ".claude-plugin")), ["plugin.json"]);
  for (const d of ["skills", "hooks", "scripts"]) assert.ok(fs.statSync(path.join(pluginDir, d)).isDirectory(), d);
  assert.ok(!fs.existsSync(path.join(pluginDir, "CLAUDE.md")), "a CLAUDE.md at the plugin root is not loaded and triggers a warning");
});

test("marketplace.json: name, owner, plugins, relative source that exists", () => {
  assert.equal(marketplace.name, "graft-local");
  assert.doesNotMatch(marketplace.name, /^(claude|anthropic)|official/i);
  assert.ok(marketplace.owner && typeof marketplace.owner.name === "string");
  assert.ok(Array.isArray(marketplace.plugins) && marketplace.plugins.length === 1);
  const entry = marketplace.plugins[0];
  assert.equal(entry.name, manifest.name, "entry name must equal the manifest name");
  assert.match(entry.source, /^\.\//);
  assert.ok(!entry.source.includes(".."));
  assert.ok(fs.existsSync(path.join(root, entry.source, ".claude-plugin/plugin.json")));
  assert.equal(path.resolve(root, entry.source), pluginDir);
});

test("skills: one folder per slash command, valid frontmatter", () => {
  const dirs = fs.readdirSync(path.join(pluginDir, "skills")).sort();
  assert.deepEqual(dirs, EXPECTED_SKILLS);
  for (const d of dirs) {
    const file = path.join(pluginDir, "skills", d, "SKILL.md");
    const { fields, body } = frontmatter(file);
    assert.equal(fields.name, d, `${d}: name must match the folder`);
    assert.ok(fields.description && fields.description.length > 30, `${d}: description`);
    for (const k of Object.keys(fields)) assert.ok(SKILL_FIELDS.has(k), `${d}: unknown frontmatter field ${k}`);
    if ("disable-model-invocation" in fields) assert.ok(["true", "false"].includes(fields["disable-model-invocation"]), d);
    assert.ok(fields["allowed-tools"], `${d}: allowed-tools`);
    // the only tool a command may pre approve is the plugin wrapper
    assert.equal(fields["allowed-tools"], "Bash(${CLAUDE_PLUGIN_ROOT}/scripts/re.sh *)", `${d}: allowed-tools must be the wrapper only`);
    assert.ok(body.includes("${CLAUDE_PLUGIN_ROOT}/scripts/re.sh"), `${d}: body must call the wrapper`);
    // every command in the body calls the wrapper, never the binary directly
    assert.doesNotMatch(body, /^\s*(\$\s*)?graft\s/m, `${d}: call re.sh, not the binary`);
  }
});

test("skills: commands that change state are not model invocable; read only ones may be", () => {
  const mutating = ["apply", "evolve", "propose", "init", "export"];
  for (const d of mutating) {
    const { fields } = frontmatter(path.join(pluginDir, "skills", d, "SKILL.md"));
    assert.equal(fields["disable-model-invocation"], "true", `${d} must be user invoked only`);
  }
  for (const d of ["status", "digest", "verify-log"]) {
    const { fields } = frontmatter(path.join(pluginDir, "skills", d, "SKILL.md"));
    assert.notEqual(fields["disable-model-invocation"], "false");
  }
});

test("skills: evolve and propose never allow a paid provider or direct rules edits", () => {
  const evolve = frontmatter(path.join(pluginDir, "skills/evolve/SKILL.md")).body;
  assert.match(evolve, /--provider mock/);
  assert.match(evolve, /Never pass any other provider/);
  assert.doesNotMatch(evolve, /--provider (openai|anthropic|real)/);
  const propose = frontmatter(path.join(pluginDir, "skills/propose/SKILL.md")).body;
  assert.match(propose, /Never edit the rules file/);
  const apply = frontmatter(path.join(pluginDir, "skills/apply/SKILL.md")).body;
  assert.match(apply, /re\.sh apply --version <version or current> --json/);
  assert.match(apply, /ask them to confirm/);
  assert.match(apply, /Do not pass any provider option/);
  assert.doesNotMatch(apply, /--provider/);
});

test("skills: every subcommand they call exists in the CLI", () => {
  const src = fs.readFileSync(path.join(repo, "crates/cli/src/main.rs"), "utf8") +
    walk(path.join(repo, "crates/cli/src")).map(read).join("\n");
  for (const d of EXPECTED_SKILLS) {
    const { body } = frontmatter(path.join(pluginDir, "skills", d, "SKILL.md"));
    const calls = [...body.matchAll(/re\.sh (?:-C \S+ )?([a-z][a-z-]*)/g)].map((m) => m[1]);
    assert.ok(calls.length > 0, `${d}: no re.sh call found`);
    for (const c of calls) {
      const camel = c.split("-").map((w) => w[0].toUpperCase() + w.slice(1)).join("");
      assert.ok(new RegExp(`\\b${camel}\\b`).test(src), `${d}: '${c}' is not a CLI subcommand`);
    }
  }
});

test("skills: flags they use exist in the CLI help (when a binary is built)", (t) => {
  const bin = process.env.GRAFT_BIN || ["release", "debug"].map((k) => path.join(repo, "target", k, "graft")).find(fs.existsSync);
  if (!bin) return t.skip("no built graft binary");
  const help = (sub) => execFileSync(bin, [sub, "--help"], { encoding: "utf8" });
  const wanted = {
    evolve: ["--generations", "--seed", "--provider", "--replay-file", "--json"],
    propose: ["--file", "--hypothesis", "--json"],
    apply: ["--version", "--yes", "--force", "--json"],
    init: ["--model", "--rules-file", "--json"],
    export: ["--out", "--json"],
    status: ["--json"],
    "verify-log": ["--json"],
    digest: ["--json"],
  };
  for (const [sub, flags] of Object.entries(wanted)) {
    const h = help(sub);
    for (const f of flags) assert.ok(h.includes(f), `${sub} --help lacks ${f}`);
  }
});

test("hooks.json: wrapper key, known events, command hooks on plugin scripts", () => {
  assert.deepEqual(Object.keys(hooksFile).sort(), ["description", "hooks"]);
  const events = Object.keys(hooksFile.hooks);
  assert.deepEqual(events.sort(), ["PostToolUse", "SessionStart"]);
  for (const ev of events) {
    assert.ok(HOOK_EVENTS.has(ev), `unknown event ${ev}`);
    for (const group of hooksFile.hooks[ev]) {
      assert.ok(Array.isArray(group.hooks) && group.hooks.length > 0);
      for (const h of group.hooks) {
        assert.equal(h.type, "command");
        assert.match(h.command, /^\$\{CLAUDE_PLUGIN_ROOT\}\/scripts\/[a-z-]+\.sh$/);
        assert.deepEqual(h.args, [], "exec form, so the path is one argument");
        assert.ok(Number.isInteger(h.timeout) && h.timeout > 0 && h.timeout <= 30, "short timeout");
        assert.equal(h.onFailure, undefined, "a hook failure must never block");
        const script = path.join(pluginDir, h.command.replace("${CLAUDE_PLUGIN_ROOT}/", ""));
        assert.ok(fs.existsSync(script), `${script} exists`);
        fs.accessSync(script, fs.constants.X_OK);
        assert.match(read(script), /^#!\/bin\/sh\n/);
      }
    }
  }
  // SessionStart matcher values are documented: startup, resume, clear, compact, fork
  const m = hooksFile.hooks.SessionStart[0].matcher.split("|");
  for (const v of m) assert.ok(["startup", "resume", "clear", "compact", "fork"].includes(v), v);
  // PostToolUse matcher is exact tool names
  assert.equal(hooksFile.hooks.PostToolUse[0].matcher, "Edit|Write|MultiEdit");
});

test("hook scripts never block: no exit 2, no deny or block decisions", () => {
  for (const f of ["session-start.sh", "edit-warn.sh"]) {
    const text = read(path.join(pluginDir, "scripts", f));
    assert.doesNotMatch(text, /exit\s+2\b/, `${f}: exit 2 would block`);
    assert.doesNotMatch(text, /permissionDecision|"decision"|"continue"|set -e/, f);
    assert.ok(text.trimEnd().endsWith("exit 0"), `${f}: must end with exit 0`);
    assert.doesNotMatch(text, /\b(curl|wget|nc|ssh)\b/, `${f}: no network tools`);
  }
});

test("scripts do not call a model or the network", () => {
  for (const f of walk(path.join(pluginDir, "scripts"))) {
    const text = read(f);
    assert.doesNotMatch(text, /https?:\/\/|ANTHROPIC_API_KEY|OPENAI_API_KEY|api\.anthropic/i, f);
  }
});

test("no em dashes or en dashes in any plugin, test or doc file", () => {
  const files = [...walk(root), path.join(repo, "README.md")];
  for (const f of files) {
    const text = read(f);
    assert.ok(!/[\u2013\u2014]/.test(text), `${path.relative(repo, f)} contains an em or en dash`);
  }
});

test("all plugin json files are valid and use two space indentation", () => {
  for (const f of walk(root).filter((p) => p.endsWith(".json"))) {
    const text = read(f);
    assert.equal(text, JSON.stringify(JSON.parse(text), null, 2) + "\n", `${path.relative(repo, f)} is not canonical`);
  }
});

test("README states v4.x and keeps the Petri credit", () => {
  const readme = read(path.join(repo, "README.md"));
  assert.match(readme, /^## Status: v4\.\d+\.\d+$/m);
  assert.match(readme, /https:\/\/ethglobal\.com\/showcase\/petri-mjy1y/);
  assert.match(readme, /## Claude Code plugin/);
  for (const s of EXPECTED_SKILLS) assert.ok(readme.includes(`/graft:${s}`), `README lacks /graft:${s}`);
  assert.match(readme, /GRAFT_HOOKS=off/);
  assert.match(readme, /hooks\.off/);
});
