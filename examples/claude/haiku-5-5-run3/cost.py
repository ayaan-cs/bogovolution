#!/usr/bin/env python3
"""Cumulative cost of every paid call made for runs 3 and later.

Sums the provider's usage in every transcript under
$WORKDIR/claude3/transcripts/ and $WORKDIR/claude4/transcripts/.
Prices are per million tokens (input, output), checked on the Anthropic
pricing page before use. An unknown model is priced at the highest known
price so the guard errs on the safe side."""
import glob, json, os, sys
PRICES = {
    "claude-haiku-5-5": (0.10, 0.50),
    "claude-sonnet-5-5": (2.00, 10.00),
}
WORST = (15.0, 75.0)
def total(verbose=False):
    cost = 0.0
    rows = {}
    for f in sorted(glob.glob(os.environ.get("WORKDIR", ".") + "/claude[34]/transcripts/*.jsonl")):
        c = i = o = n = 0
        for line in open(f):
            try:
                t = json.loads(line)
            except ValueError:
                continue
            u = t.get("usage") or {}
            pi, po = PRICES.get(t.get("model", ""), WORST)
            ti = u.get("input_tokens", 0) + u.get("cache_creation_input_tokens", 0) + u.get("cache_read_input_tokens", 0)
            to = u.get("output_tokens", 0)
            c += ti * pi / 1e6 + to * po / 1e6
            i += ti; o += to; n += 1
        rows[f] = (n, i, o, c)
        cost += c
    if verbose:
        for f, (n, i, o, c) in rows.items():
            print(f"{f}: {n} calls, {i} in, {o} out, ${c:.4f}")
    return cost
if __name__ == "__main__":
    c = total("-v" in sys.argv)
    print(f"{c:.4f}")
