# Rules lineage digest

Rules file: `AGENTS.md`. Model: `demo-model`.

9 versions and 8 mutations tried: 2 accepted, 5 rejected, 1 noise, 0 pending. 2 refused before they became versions.

Current version: `cd28eb296938` (root `8e28956afd69`). Median pass rate moved from 25% to 92% (+66.7 points).

## What worked

- **Telling the agent to run the test suite before it finishes will catch more mistakes** (`de7a8ac7ef75`, +33.3 points, 5 of 5 paired wins). Median pass rate 25% to 58%.
- **Reading the failing test before editing will point the agent at the real problem** (`cd28eb296938`, +33.3 points, 5 of 5 paired wins). Median pass rate 58% to 92%.

## What failed

### Rejected

- A short summary at the end will make results easier to check (`114c80936c0c`, +0.0 points). child median 0.250 is not above parent median 0.250
- Telling the agent to run the test suite before it finishes will catch more mistakes (`e29bea8e8b9b`, +0.0 points). child median 0.583 is not above parent median 0.583
- Skipping tests will make the agent faster (`ec12bc56d472`, -41.7 points). child median 0.500 is not above parent median 0.917
- Reading the failing test before editing will point the agent at the real problem (`f04533a9c71c`, +0.0 points). child median 0.917 is not above parent median 0.917
- Telling the agent to run the test suite before it finishes will catch more mistakes (`a7d10bbfd330`, +0.0 points). child median 0.917 is not above parent median 0.917

### Refused

- Skipping tests will make the agent faster: this content already exists in the tree as ec12bc56d472
- Reading the failing test before editing will point the agent at the real problem: this content already exists in the tree as f04533a9c71c

## Noise

- Asking for small commits might help a little (`cc79f0cf48b7`, +8.3 points, 3 of 5 paired wins). median gain 8.3 points is below 10 and the child won 3 of 5 pairs (needs 4)

## Pending

Nothing is waiting for a verdict.

## Directions not tried yet

- Retest "Asking for small commits might help a little" with more repeats. It gained 8.3 points but won 3 of 5 pairs, which was not clear enough.
- Build on the latest win, "Reading the failing test before editing will point the agent at the real problem" (+33.3 points). Try a follow up that extends it instead of repeating it.
- Avoid rules around: check, easier, faster, result, short, unless you change the angle. These words only show up in rejected changes.
- 2 attempts were refused as repeats of text already in the tree. Offer the proposer a wider range of changes.
- Try a rule about checking its own work: ask the agent to review its own diff before finishing.
- Try a rule about errors and edge cases: ask the agent to think about failures and edge cases.
- Try a rule about asking when unsure: ask the agent to state assumptions or ask when a task is unclear.
- Try a rule about code style: point the agent at the project's style and naming conventions.
- Try a rule about documentation: say when to update docs and comments.

These suggestions come from fixed rules over the history above. No model wrote them.
