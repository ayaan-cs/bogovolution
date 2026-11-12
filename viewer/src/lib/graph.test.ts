import { describe, expect, it } from 'vitest';
import demo from '../demo/tree.json';
import { buildElements, nodeLabel, refusedShownOnLoad } from './graph';
import type { LineageExport } from './types';

const d = demo as unknown as LineageExport;

describe('buildElements', () => {
  const els = buildElements(d);
  const nodes = els.filter((e) => e.group === 'nodes');
  const edges = els.filter((e) => e.group === 'edges');

  it('draws one node per version and one edge per change, with refused mutations counted instead', () => {
    expect(nodes).toHaveLength(d.nodes.length);
    expect(edges).toHaveLength(d.edges.length);
    expect(nodes.some((n) => n.data.kind === 'refused')).toBe(false);
    const ids = new Set(nodes.map((n) => n.data.id));
    for (const e of edges) {
      expect(ids.has(e.data.source)).toBe(true);
      expect(ids.has(e.data.target)).toBe(true);
    }
  });

  it('puts the refusal count and the gain on the node', () => {
    const parent = nodes.find((n) => n.data.id === d.refused[0].parent);
    expect(parent?.data.label).toContain('2 refused');
    const accepted = nodes.filter((n) => n.data.status === 'accepted');
    expect(accepted.every((n) => /\+33 pts/.test(n.data.label))).toBe(true);
    const root = nodes.find((n) => n.data.status === 'root');
    expect(root?.data.label).toBe('8e28956\nroot \u00b7 25%');
  });

  it('gives taller boxes to nodes with more lines', () => {
    const root = nodes.find((n) => n.data.status === 'root');
    const parent = nodes.find((n) => n.data.id === d.refused[0].parent);
    expect(Number(parent?.data.h)).toBeGreaterThan(Number(root?.data.h));
  });

  it('can still draw refused mutations as small dashed nodes', () => {
    const all = buildElements(d, { showRefused: true });
    const n = all.filter((e) => e.group === 'nodes');
    expect(n).toHaveLength(d.nodes.length + d.refused.length);
    expect(n.filter((x) => x.data.kind === 'refused')).toHaveLength(2);
    const ids = new Set(n.map((x) => x.data.id));
    for (const e of all.filter((x) => x.group === 'edges')) {
      expect(ids.has(e.data.source)).toBe(true);
      expect(ids.has(e.data.target)).toBe(true);
    }
  });

  it('colors by verdict through the status field', () => {
    const statuses = new Set(nodes.map((n) => n.data.status));
    for (const s of ['root', 'accepted', 'rejected', 'noise']) expect(statuses.has(s)).toBe(true);
    expect(edges.filter((e) => e.data.status === 'accepted')).toHaveLength(2);
  });

  it('marks exactly one node as current', () => {
    expect(nodes.filter((n) => n.data.current === 1)).toHaveLength(1);
    expect(nodes.find((n) => n.data.current === 1)?.data.id).toBe(d.current);
  });

  it('labels nodes with id, status, median, gain and refusals', () => {
    expect(nodeLabel('accepted', 'abc1234', 0.92)).toBe('abc1234\naccepted \u00b7 92%');
    expect(nodeLabel('pending', 'abc1234', null)).toBe('abc1234\npending \u00b7 no runs');
    expect(nodeLabel('overfit', 'abc1234', 0.75, 25)).toBe('abc1234\noverfit \u00b7 75%\n+25 pts');
    expect(nodeLabel('rejected', 'abc1234', 0.5, -41.7, 3)).toBe('abc1234\nrejected \u00b7 50%\n-42 pts\n3 refused');
  });
});

describe('refusedShownOnLoad', () => {
  it('hides refused nodes after loading a file', () => {
    expect(refusedShownOnLoad(null)).toBe(false);
    expect(refusedShownOnLoad({ kind: 'node', hash: 'abc' })).toBe(false);
  });
  it('shows them when the file opens on a refused item', () => {
    expect(refusedShownOnLoad({ kind: 'refused', seq: 41 })).toBe(true);
  });
});
