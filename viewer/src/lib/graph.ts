import type { ElementDefinition } from 'cytoscape';
import { points, pct, shortHash, statusLabel } from './format';
import { nodeHeight } from './layout';
import type { LineageExport } from './types';

/// Lines in a node: the short id, the status and median, then optionally the gain
/// over the parent ("+33 pts") and the number of refused attempts made from this version.
export function nodeLabel(
  status: string,
  shortId: string,
  median: number | null,
  gain: number | null = null,
  refused = 0,
): string {
  const lines = [shortId, `${statusLabel(status as never)} \u00b7 ${pct(median)}`];
  if (gain !== null) lines.push(points(gain));
  if (refused > 0) lines.push(`${refused} refused`);
  return lines.join('\n');
}

export interface BuildOptions {
  /// Draw each refused mutation as a small dashed node. Off by default: they are a count on
  /// the parent and listed in the details pane, which keeps the rows of the tree narrow.
  showRefused?: boolean;
}

/// Cytoscape elements for the lineage: one node per version and one edge per proposed
/// change. Refused mutations are counted on their parent, or drawn when `showRefused` is set.
export function buildElements(d: LineageExport, opts: BuildOptions = {}): ElementDefinition[] {
  const els: ElementDefinition[] = [];
  const refusedBy = new Map<string, number>();
  for (const r of d.refused) refusedBy.set(r.parent, (refusedBy.get(r.parent) ?? 0) + 1);
  const gainOf = new Map<string, number | null>();
  for (const e of d.edges) gainOf.set(e.child, e.gain_pp);
  for (const n of d.nodes) {
    const gain = gainOf.get(n.hash) ?? null;
    const refused = refusedBy.get(n.hash) ?? 0;
    const lines = 2 + (gain !== null ? 1 : 0) + (refused > 0 ? 1 : 0);
    els.push({
      group: 'nodes',
      data: {
        id: n.hash,
        label: nodeLabel(n.status, shortHash(n.hash), n.median_pass_rate, gain, refused),
        status: n.status,
        current: n.is_current ? 1 : 0,
        kind: 'version',
        h: nodeHeight(lines),
      },
    });
  }
  for (const e of d.edges) {
    els.push({
      group: 'edges',
      data: { id: `e-${e.seq}`, source: e.parent, target: e.child, status: e.verdict ?? 'pending' },
    });
  }
  if (opts.showRefused) {
    for (const r of d.refused) {
      els.push({
        group: 'nodes',
        data: { id: `refused-${r.seq}`, label: 'refused', status: 'refused', current: 0, kind: 'refused', h: 26 },
      });
      els.push({
        group: 'edges',
        data: { id: `re-${r.seq}`, source: r.parent, target: `refused-${r.seq}`, status: 'refused' },
      });
    }
  }
  return els;
}
