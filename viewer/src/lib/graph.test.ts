import { describe, expect, it } from 'vitest';
import demo from '../demo/tree.json';
import { buildElements, nodeLabel } from './graph';
import type { LineageExport } from './types';

const d = demo as unknown as LineageExport;

describe('buildElements', () => {
  const els = buildElements(d);
  const nodes = els.filter((e) => e.group === 'nodes');
  const edges = els.filter((e) => e.group === 'edges');

  it('draws one node per version plus one per refused mutation', () => {
    expect(nodes).toHaveLength(d.nodes.length + d.refused.length);
    expect(nodes.filter((n) => n.data.kind === 'refused')).toHaveLength(2);
  });

  it('draws one edge per change plus one per refusal, all pointing at real nodes', () => {
    expect(edges).toHaveLength(d.edges.length + d.refused.length);
    const ids = new Set(nodes.map((n) => n.data.id));
    for (const e of edges) {
      expect(ids.has(e.data.source)).toBe(true);
      expect(ids.has(e.data.target)).toBe(true);
    }
  });

  it('colors by verdict through the status field', () => {
    const statuses = new Set(nodes.map((n) => n.data.status));
    for (const s of ['root', 'accepted', 'rejected', 'noise', 'refused']) expect(statuses.has(s)).toBe(true);
    const accepted = edges.filter((e) => e.data.status === 'accepted');
    expect(accepted).toHaveLength(2);
    expect(accepted.every((e) => e.data.label === 'pass rate\n+33 pts')).toBe(true);
  });

  it('marks exactly one node as current', () => {
    expect(nodes.filter((n) => n.data.current === 1)).toHaveLength(1);
    expect(nodes.find((n) => n.data.current === 1)?.data.id).toBe(d.current);
  });

  it('labels nodes with id, status and median pass rate', () => {
    expect(nodeLabel('accepted', 'abc1234', 0.92)).toBe('abc1234\naccepted \u00b7 92%');
    expect(nodeLabel('pending', 'abc1234', null)).toBe('abc1234\npending \u00b7 no runs');
  });
});
