// Writes test-fixtures/wide-tree.json: a synthetic lineage with 32 versions and 8 children on
// more than one parent, used to check that the graph stays readable on wide trees.
//   node scripts/make-wide-fixture.mjs
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const demo = JSON.parse(readFileSync(join(root, 'src', 'demo', 'tree.json'), 'utf8'));
const hash = (i) => createHash('sha256').update(`wide-${i}`).digest('hex');

let seq = 10;
const nodes = [];
const edges = [];
const refused = [];
const series = [];
const rates = {};

function runs(rate) {
  return [0, 1, 2].map((i) => ({ run_index: i, seed: 1000 + i, passed: Math.round(rate * 12), total: 12, pass_rate: Math.round(rate * 12) / 12, tokens_used: 1000, budget_exhausted: false }));
}
function addNode(i, parent, status, rate, gen, hyp) {
  const h = hash(i);
  nodes.push({
    hash: h, short: h.slice(0, 12), parent: parent === null ? null : hash(parent), generation: gen, created_seq: seq,
    hypothesis: hyp, verdict: status === 'root' || status === 'pending' ? null : status, status, is_current: false,
    size_bytes: 100 + i, content: `# Project rules\n\nVersion ${i}\n`, diff: parent === null ? null : `diff --git a/AGENTS.md b/AGENTS.md\n--- a/AGENTS.md\n+++ b/AGENTS.md\n@@ -1,3 +1,3 @@\n # Project rules\n \n-Version ${parent}\n+Version ${i}\n`,
    median_pass_rate: Math.round(rate * 12) / 12, runs: runs(rate),
  });
  if (parent !== null) {
    const pr = rates[parent];
    const gain = (Math.round(rate * 12) / 12 - Math.round(pr * 12) / 12) * 100;
    edges.push({
      parent: hash(parent), child: h, seq, hypothesis: hyp, verdict: status, reason: `median gain ${gain.toFixed(1)} points`,
      parent_median: Math.round(pr * 12) / 12, child_median: Math.round(rate * 12) / 12, gain_pp: gain, paired_wins: status === 'accepted' ? 5 : 1, repeats: 5, pairs: [],
    });
    series.push({
      attempt: 0, seq, outcome: status, parent: hash(parent), child: h, hypothesis: hyp,
      parent_median: Math.round(pr * 12) / 12, child_median: Math.round(rate * 12) / 12, gain_pp: gain, current_after: h, current_median_after: Math.round(rate * 12) / 12,
    });
  }
  rates[i] = rate;
  seq += 1;
}
const kids = (parent, first, n, gen, accepted) => {
  for (let k = 0; k < n; k++) {
    const i = first + k;
    const status = k === 0 ? 'accepted' : k % 3 === 0 ? 'noise' : 'rejected';
    const base = rates[parent];
    const rate = status === 'accepted' ? Math.min(1, base + 0.25) : status === 'noise' ? Math.min(1, base + 0.08) : Math.max(0, base - 0.1);
    addNode(i, parent, status, rate, gen, `Try variation ${i} of the rules (${status}).`);
  }
  return accepted;
};
addNode(0, null, 'root', 0.25, 0, null);
kids(0, 1, 8, 1);       // 8 children on the root
kids(1, 9, 8, 2);       // 8 children on the first accepted child
kids(9, 17, 6, 3);      // 6 more on its accepted child
kids(17, 23, 5, 4);
kids(23, 28, 4, 5);
// A second branch from the root's other children.
kids(4, 32, 2, 2);
// Refusals.
for (const [p, n] of [[1, 3], [9, 2], [17, 1]]) {
  for (let k = 0; k < n; k++) {
    refused.push({ seq, parent: hash(p), hypothesis: `Refused idea ${p}-${k}`, reason: 'this content already exists in the tree', seed: null, provider: 'mock' });
    series.push({ attempt: 0, seq, outcome: 'refused', parent: hash(p), child: null, hypothesis: `Refused idea ${p}-${k}`, parent_median: null, child_median: null, gain_pp: null, current_after: hash(p), current_median_after: rates[p] });
    seq += 1;
  }
}
series.sort((a, b) => a.seq - b.seq).forEach((s, i) => (s.attempt = i + 1));
const current = 28;
nodes.find((n) => n.hash === hash(current)).is_current = true;
const count = (s) => nodes.filter((n) => n.status === s).length;
const out = {
  ...demo,
  project: { rules_file: 'AGENTS.md', model: 'wide-fixture' },
  root: hash(0), current: hash(current),
  summary: { nodes: nodes.length, edges: edges.length, accepted: count('accepted'), rejected: count('rejected'), noise: count('noise'), pending: 0, refused: refused.length, runs: nodes.length * 3, tokens_used: nodes.length * 3000, max_generation: 5 },
  nodes, edges, refused, series,
};
writeFileSync(join(root, 'test-fixtures', 'wide-tree.json'), JSON.stringify(out, null, 1) + '\n');
console.log('wrote test-fixtures/wide-tree.json', nodes.length, 'versions');
