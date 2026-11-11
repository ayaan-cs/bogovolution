import type { LineageExport, NodeStatus, Outcome, TreeEdge, TreeNode } from './types';

export function pct(rate: number | null | undefined): string {
  return rate === null || rate === undefined ? 'no runs' : `${Math.round(rate * 100)}%`;
}

/// "+33 pts", "-42 pts", "0 pts". Rounds to whole points.
export function points(gain: number | null | undefined): string {
  if (gain === null || gain === undefined) return 'n/a';
  const r = Math.round(gain);
  if (r === 0) return '0 pts';
  return `${r > 0 ? '+' : '-'}${Math.abs(r)} pts`;
}

export function statusLabel(s: NodeStatus | Outcome): string {
  return s === 'pending' ? 'pending' : s;
}

export function shortHash(hash: string, n = 7): string {
  return hash.slice(0, n);
}

/// Short sentence for the verdict of a change.
export function verdictSummary(e: TreeEdge, cfg: LineageExport['config']): string {
  if (e.verdict === null) return 'No verdict yet. Run both versions and verify.';
  if (e.reason) return e.reason;
  return `Needs a gain of ${cfg.accept_threshold_pp} points and ${cfg.min_paired_wins} paired wins.`;
}

export function nodeById(d: LineageExport, hash: string): TreeNode | undefined {
  return d.nodes.find((n) => n.hash === hash);
}

export function edgeForChild(d: LineageExport, hash: string): TreeEdge | undefined {
  return d.edges.find((e) => e.child === hash);
}

/// Find a version by hash prefix. Needs a unique match of at least 4 characters.
export function resolvePrefix(d: LineageExport, prefix: string): TreeNode | undefined {
  if (prefix.length < 4) return undefined;
  const hits = d.nodes.filter((n) => n.hash.startsWith(prefix.toLowerCase()));
  return hits.length === 1 ? hits[0] : undefined;
}
