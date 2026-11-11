import { describe, expect, it } from 'vitest';
import demo from '../demo/tree.json';
import { edgeForChild, nodeById, pct, points, resolvePrefix, shortHash, verdictSummary } from './format';
import type { LineageExport, TreeEdge } from './types';

const d = demo as unknown as LineageExport;

describe('format', () => {
  it('rounds points and signs them', () => {
    expect(points(33.33)).toBe('+33 pts');
    expect(points(-41.7)).toBe('-42 pts');
    expect(points(0)).toBe('0 pts');
    expect(points(-0.2)).toBe('0 pts');
    expect(points(null)).toBe('n/a');
  });

  it('formats pass rates', () => {
    expect(pct(0.9167)).toBe('92%');
    expect(pct(0)).toBe('0%');
    expect(pct(null)).toBe('no runs');
  });

  it('explains verdicts', () => {
    const e = d.edges[0];
    expect(verdictSummary(e, d.config)).toBe(e.reason);
    expect(verdictSummary({ ...e, verdict: null }, d.config)).toMatch(/No verdict yet/);
  });

  it('finds nodes and edges', () => {
    const child = d.nodes[1];
    expect(nodeById(d, child.hash)).toBe(child);
    expect(edgeForChild(d, child.hash)?.child).toBe(child.hash);
    expect(edgeForChild(d, d.root)).toBeUndefined();
  });

  it('resolves hash prefixes only when they are unique and long enough', () => {
    const n = d.nodes[3];
    expect(resolvePrefix(d, n.hash.slice(0, 8))).toBe(n);
    expect(resolvePrefix(d, n.hash.slice(0, 3))).toBeUndefined();
    expect(resolvePrefix(d, 'zzzzzz')).toBeUndefined();
    expect(shortHash(n.hash)).toBe(n.hash.slice(0, 7));
  });
});
