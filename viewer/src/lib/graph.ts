import type { ElementDefinition } from 'cytoscape';
import { edgeLabel, pct, shortHash, statusLabel } from './format';
import type { LineageExport } from './types';

export function nodeLabel(status: string, shortId: string, median: number | null): string {
  return `${shortId}\n${statusLabel(status as never)} \u00b7 ${pct(median)}`;
}

/// Cytoscape elements for the lineage: one node per version, one edge per
/// proposed change, and a small dashed node for each refused mutation.
export function buildElements(d: LineageExport): ElementDefinition[] {
  const els: ElementDefinition[] = [];
  for (const n of d.nodes) {
    els.push({
      group: 'nodes',
      data: {
        id: n.hash,
        label: nodeLabel(n.status, shortHash(n.hash), n.median_pass_rate),
        status: n.status,
        current: n.is_current ? 1 : 0,
        kind: 'version',
      },
    });
  }
  for (const e of d.edges) {
    els.push({
      group: 'edges',
      data: {
        id: `e-${e.seq}`,
        source: e.parent,
        target: e.child,
        label: edgeLabel(e),
        status: e.verdict ?? 'pending',
      },
    });
  }
  for (const r of d.refused) {
    els.push({
      group: 'nodes',
      data: { id: `refused-${r.seq}`, label: 'refused', status: 'refused', current: 0, kind: 'refused' },
    });
    els.push({
      group: 'edges',
      data: { id: `re-${r.seq}`, source: r.parent, target: `refused-${r.seq}`, label: '', status: 'refused' },
    });
  }
  return els;
}
