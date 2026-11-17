# Dev note, 2026-11-17: testing the evolve loop after six rounds

Six rounds of real model runs (Qwen2.5 Coder 3B and 7B, local, free) turned
up a series of problems in the evolve loop itself, separate from the question
of whether a model can improve its rules. This note lists them, the fix for
each, and the tests that now pin them down (v2.7.2). Everything here ran
locally with the mock model or replays. No paid API, nothing published.

The pictures live outside the repo, in
`<workdir>/rules-evolve-media/dev-note-2026-11-17/` (the links below are
relative to this file on the build machine). They are not committed.

## Timeline

| Round | Date (CT) | Setup | Result | Commit |
| --- | --- | --- | --- | --- |
| 1 | Nov 11 | 3B, 8 tasks | Null: every version 37.5%, even the hand written control. The model could not follow a correct rule. Outputs not shipped (research only license). | c8b1ecf |
| 2 | Nov 11 | 7B, 8 tasks, failure notes in the prompt | The `islice` rule accepted, +25 points, 5 of 5 wins. Control also +25. Gen 3 wasted: identical to its parent. | d184c91, 31d1a48 |
| 3 | Nov 11 | Same rules on 8 held out tasks | The `islice` rule was overfit: 37.5% held out against 50% for the root. Control +12.5. Led to the overfit verdict. | fa7f653, 8a5c6cf |
| 4 | Nov 13 | 7B, 8 train + 8 held out, one evolve | Nothing accepted. Gen 3 wasted: already in the tree. | 0c1eed2 |
| 5 | Nov 15 to 16 | 7B, 24 + 24 tasks, every stub in the prompt | Nothing moved; 2 runs per candidate because of the 13 hour cost. | 030a969, d0d1424 |
| 6 | Nov 16 | 7B, 24 + 24, short prompts, 5 runs each | Nothing accepted, nothing generalized. Gen 3 first try invalid (server killed for memory), rerun rejected. | bab05b3, afcf50e, 650aa94 |
| 7 | Nov 17 | This note: hardening and tests, no model runs | 22 new tests, 4 behaviour fixes | this commit (v2.7.2) |

![Median pass rate per version across rounds](../../../rules-evolve-media/dev-note-2026-11-17/chart-rounds.png)

## Issues and fixes

1. **A dead model server became a fake verdict.** At 3:27 PM CT on Nov 16
   the kernel killed llama-server for memory 22 calls into round 6 gen 3.
   Every later call failed to connect, each failure was counted as a failed
   task, and the generation was logged as rejected at -37.5 points.
   Workaround then: marked invalid in the docs and rerun (afcf50e, 650aa94),
   plus `--cache-ram 1024` on the server. Fix now (v2.7.2): a failed model
   call stops the run with a clear error, the run is not recorded and
   nothing is counted. `evolve` run again picks up where it stopped.
   `[evaluation] max_model_errors_per_run` keeps the old tolerance as an
   opt in.
2. **verify-log called a valid log edited.** Medians such as 5/24 and 10/24
   did not read back to the same float, so the recomputed hash differed and
   appending was refused. Fixed in v2.7.1 (afcf50e, serde_json
   `float_roundtrip`). Now also covered end to end by an evolve run whose
   verdicts carry those medians; the test fails without the fix (checked by
   turning the feature off).
3. **Wasted generations on repeats.** Round 2 gen 3 proposed its parent's
   exact text and round 4 gen 3 a version already in the tree; both
   generations were spent on a refusal. The mutation seed already changes
   with every attempt (it is derived from the number of earlier attempts),
   so this was the model repeating itself, not a fixed seed. Note: the
   round 6 gen 3 rerun repeating the first gen 3 try was intended (same
   attempt count, same seed, same prompt). Fix now: `[mutation]
   retries_on_refusal = N` asks again with the next seed after a refused
   answer, up to N times, and a change that differs only in whitespace is
   refused too. Default 0, so every committed replay is unchanged.
4. **The token cap cut runs short quietly.** A capped run counted the tasks
   it never did as failed and the remaining runs still started. Fix now: the
   version stops after the capped run, `run` prints "STOPPED AT THE TOKEN
   CAP" with the numbers, `verify` refuses to judge capped runs, and
   `evolve` stops with the same note.
5. **Interrupted sessions.** A crash mid write can leave half a line in
   `log.jsonl` or `HEAD` one event behind; a crash between proposing and
   judging a child left the child pending. Fix now: before any write the log
   is repaired only in these exact cases (half line moved to
   `log.jsonl.partial`, missing newline added, `HEAD` one event behind
   updated). Tampering or anything else is left alone and still fails
   `verify-log`. `evolve` finishes an unjudged child first instead of
   asking for a new mutation.
6. **Memory.** llama.cpp's prompt cache defaults to 8 GiB; the server grew
   to about 12 GB. All later runs use `--cache-ram 1024` (about 6.2 GB).
   Recorded in `examples/local-model/` (afcf50e).

## New tests

`crates/core/tests/evolve_hardening.rs` (20) and `crates/cli/tests/cli.rs`
(2). All use the deterministic mock model, a scripted mutation model, or a
fake local HTTP server.

| Test | What it pins down |
| --- | --- |
| `a_repeated_mutation_is_retried_with_a_new_seed_until_it_is_new` | Repeats are refused, logged, and retried with new seeds (issue 3) |
| `without_retries_a_repeat_ends_the_generation_as_before` | Default behaviour unchanged |
| `whitespace_only_changes_are_refused` | New whitespace refusal |
| `unreadable_empty_and_oversized_answers_are_refused_and_retried` | Malformed, empty and oversized model output |
| `token_cap_refusals_are_not_retried` | No retry loop that burns tokens |
| `a_replay_recorded_before_retries_still_replays_with_retries_on` | Old replays still work with retries on |
| `a_dead_server_stops_the_run_records_nothing_and_evolve_resumes_to_the_same_result` | Model dies mid generation: nothing counted, resume gives the same lineage as an unbroken run (issue 1) |
| `tolerated_model_errors_are_recorded_as_failures_like_before` | The opt in tolerance |
| `a_local_http_server_that_dies_mid_run_leaves_no_run_behind` | Same as issue 1 through the real local provider and HTTP |
| `a_verdict_with_a_long_decimal_median_verifies_and_the_log_keeps_growing` | Float roundtrip regression (issue 2) |
| `a_capped_run_stops_the_version_says_why_and_cannot_be_judged` | Token cap (issue 4) |
| `evolve_stops_with_the_cap_note_instead_of_judging_capped_runs` | Token cap inside evolve |
| `a_half_written_last_line_is_moved_aside_and_the_run_resumes` | Crash mid write (issue 5) |
| `head_one_event_behind_is_brought_up_to_date` | Crash between log and HEAD |
| `a_whole_last_event_without_its_newline_gets_one` | Crash before the newline |
| `tampering_is_not_repaired` | Repair never hides an edit |
| `head_two_events_behind_is_not_repaired` | Repair stays narrow |
| `an_unfinished_generation_after_a_crash_is_finished_not_duplicated` | Resume does not create a second child |
| `generations_that_change_nothing_never_move_current` | All noise generations leave `current` alone |
| `positive_control_a_general_rule_is_accepted_and_a_narrow_one_is_overfit` | Positive control: on a deterministic mock a rule that lifts every task must be accepted, a rule that lifts only train tasks must be overfit, a cosmetic one rejected |
| CLI `a_capped_run_says_why_and_stops_the_version` | The cap message as the user sees it |
| CLI `a_half_written_log_line_is_repaired_and_reported_by_the_next_run` | "Log repaired" as the user sees it |

![The new tests passing](../../../rules-evolve-media/dev-note-2026-11-17/tests-passing.png)

Suite totals at this commit: `cargo test` 388 (core 240 unit + 20
hardening, CLI 57, watch 62, others 9), clippy and fmt clean, viewer and
plugin suites green (see the commit message).

## Current results (round 6, 48 tasks, 5 runs each)

| Version | Train median | Held out median | Verdict |
| --- | --- | --- | --- |
| root | 37.5% | 25.0% | |
| control (hand written) | 41.7% | 20.8% | noise, +4.2, 3 of 5; held out -4.2 |
| gen 1 | 37.5% | 25.0% | rejected, 0.0, 0 of 5 |
| gen 2 | 37.5% | 25.0% | rejected, 0.0, 1 of 5 |
| gen 3 (rerun) | 33.3% | 25.0% | rejected, -4.2, 1 of 5 |

Round 6 in the static viewer, the lineage and then gen 3 selected:

![Round 6 lineage in the viewer](../../../rules-evolve-media/dev-note-2026-11-17/viewer-round6-lineage.png)

![Round 6 gen 3 selected in the viewer](../../../rules-evolve-media/dev-note-2026-11-17/viewer-round6-gen3-selected.png)

The same log in the `watch` terminal view:

![Round 6 in rules-evolve watch](../../../rules-evolve-media/dev-note-2026-11-17/watch-round6.png)

## Still unproven

- A real Claude (or any strong model) run. Every real result so far is a
  7B or 3B local model; the paid providers were only run against a fake
  server. Whether a strong model finds general rules is the open question.
- The retry option has never run against a live model; its value on real
  repeats is assumed from rounds 2 and 4.
- The positive control proves the verdict logic on synthetic effects, not
  that 5 runs of 24 tasks can detect a real 5 to 10 point gain.
- Log repair was tested on simulated crashes, not a real power loss.

## Next steps

1. A short Claude run on the 48 task set, with the owner's go ahead on cost.
2. Turn on `retries_on_refusal = 2` for the next local run and measure how
   often it helps.
3. The deferred `islice` rule on round 6 settings.
4. More tasks or more runs to estimate the smallest gain the bar can see.
