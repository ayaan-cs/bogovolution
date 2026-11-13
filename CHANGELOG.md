# Changelog

All notable changes, newest first. Dates are in America/Chicago. Nothing has
been published: every version below exists only as a local git commit.

## Unreleased

- README: install from source, a one minute quickstart with the replay demo,
  and a short summary of what the real model runs found.
- `docs/writeup.md`: the story and the numbers of the real model runs.
- `docs/release-checklist.md`: a draft checklist for a first public release.
- The crate version now matches the documented version (2.6.0, was 0.1.0), so
  `rules-evolve --version` and the `generator.version` field of an export say
  2.6.0. The bundled demo export was regenerated for it.

## 2.6.0 (2026-11-13)

- `docker` and `podman` ran the full escape and hidden file test suites for
  the first time (Docker 29.9.3 rootful, Podman 6.2.0 rootless) and pass.
- Fix: rootless podman without cgroup delegation silently ignored
  `--pids-limit` and `--memory`. Podman now also gets `--ulimit nproc` and
  `--ulimit data`, which hold without cgroups.

## 2.5.0 (2026-11-13)

- Paid, bring your own key providers: `anthropic` (Messages API) and `openai`
  (any OpenAI compatible chat completions endpoint). They cost money on your
  own key, need `--allow-spend` and `--api-model`, read the key only from an
  environment variable, never write it anywhere, use https (or http to
  localhost only), follow no redirects, and require isolation. Tested only
  against a fake local server.
- The Claude Code plugin wrapper refuses `--allow-spend`.
- New dependency for HTTPS: `ureq` with `rustls`.

## 2.4.0 (2026-11-13)

- `apply`: writes an accepted version into the rules file. A dry run with a
  diff unless `--yes`; versions that were not accepted need `--force`; never
  writes outside the repo or through a symlink; records an `applied` event.
  Logs with an `applied` event cannot be read by older binaries.
- Claude Code plugin 2.1.0: `/rules-evolve:apply`, which shows the diff and
  asks before writing.

## 2.3.0 (2026-11-11)

- Held out tasks: `split = "held_out"` in `tasks.toml`. The mutation prompt
  never shows them. A child that wins on train but not on held out gets the new
  verdict `overfit`, and `current` does not move.
- Held out check of the 7B result: the accepted `islice` rule is overfit
  (37.5% against 50% on 8 held out tasks), while a general hand written rule
  gains on both. Replay committed.

## 2.2.0 (2026-11-11)

- Failure notes: the mutation prompt includes short notes on why tasks failed.
- Local model test with Qwen2.5 Coder 7B Instruct (Apache 2.0): one mutation
  accepted, +25 points, 5 of 5 paired wins, in two independent runs. Replays
  committed.

## 2.1.0 (2026-11-11)

- `local` provider: a free model server on this machine (llama.cpp or any
  OpenAI compatible server on loopback). Real model output, so isolation is
  required.
- First real model test (Qwen2.5 Coder 3B): nothing accepted. Its answers are
  not shipped because of the model's license.

## 2.0.0 (2026-11-11)

- Claude Code plugin with slash commands and opt in hooks. Mock and replay
  providers only.

## 1.2.4 and earlier (2026-11-11)

- 1.2.4: readable lineage graph in the viewer.
- 1.2.3: viewer chart and `watch` polish.
- 1.2.2: shadow warnings for layouts where model files could shadow hidden
  tests.
- 1.2.1: hidden test files mounted read only during the check.
- 1.2: pluggable isolation for the check command (`bwrap`, `docker`,
  `podman`, `none`, `auto`), and the rule that a real provider must run
  isolated.
- 1.1: the `watch` terminal view and replay `--pace`.
- Phases 1 to 4: the hash chained log, the benchmark harness and verdict rule,
  `mutate` and `evolve` with record and replay, `digest`, `export` with a
  versioned JSON schema, and the static viewer.

Inspired by [Petri](https://ethglobal.com/showcase/petri-mjy1y) (ETHOnline
2026).
