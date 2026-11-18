# Claude run 4: claude-sonnet-5-5, same 28 tasks as run 3

Run because Haiku plateaued on held out (about 50%). Same budget and guard
as run 3. Sonnet 5.5 costs 20 times Haiku ($2 in, $10 out per million
tokens), so this is one run of the root and one of the run 3 accepted rule,
thinking off (`--api-thinking between_tools`, its form of off; it refuses
`disabled`), effort high, cap 3,072, `max_tokens_per_run` 60,000 so one run
could cost at most about $0.60. Cost: $0.3566 for 56 calls.

| Version | Train | Held out |
|---|---|---|
| 8e28956a root | 11 of 12 (92%) | 7 of 16 (44%) |
| 1ef4720c run 3 accepted rule | 11 of 12 (92%) | 9 of 16 (56%) |

With one run each there is no verdict: `verify` refuses because
`min_paired_wins` (3) is more than 1 repeat. Sonnet's root matches Haiku's
best rule on train; the same held out tasks fail (chunked, chunked_even,
locate, circular_shifts, before_and_after, windowed). No more Sonnet calls
were made: a 3 repeat evolve would cost about $0.55 per version and the
remaining failures need undocumented behavior, so it had little chance to
raise held out honestly.

`replay.sh` replays it for free and must reproduce `log.jsonl`.

Verified on Nov 18, 2026: a free replay into a fresh project gave the same
runs, task results, token counts, hypotheses and verdicts as `log.jsonl`.
