// Shapes of schema/export.v1.json. The schema is the source of truth: the
// loader validates every file against it before these types are trusted.

export type Verdict = 'accepted' | 'rejected' | 'noise';
export type NodeStatus = 'root' | 'pending' | Verdict;
export type Outcome = Verdict | 'pending' | 'refused';

export interface Run {
  run_index: number;
  seed: number | null;
  passed: number;
  total: number;
  pass_rate: number;
  tokens_used: number;
  budget_exhausted: boolean;
}

export interface TreeNode {
  hash: string;
  short: string;
  parent: string | null;
  generation: number;
  created_seq: number;
  hypothesis: string | null;
  verdict: Verdict | null;
  status: NodeStatus;
  is_current: boolean;
  size_bytes: number;
  content: string;
  diff: string | null;
  median_pass_rate: number | null;
  runs: Run[];
}

export interface Pair {
  run_index: number;
  parent_rate: number;
  child_rate: number;
  win: boolean;
}

export interface TreeEdge {
  parent: string;
  child: string;
  seq: number;
  hypothesis: string;
  verdict: Verdict | null;
  reason: string | null;
  parent_median: number | null;
  child_median: number | null;
  gain_pp: number | null;
  paired_wins: number | null;
  repeats: number | null;
  pairs: Pair[];
}

export interface Refusal {
  seq: number;
  parent: string;
  hypothesis: string | null;
  reason: string;
  seed: number | null;
  provider: string | null;
}

export interface SeriesPoint {
  attempt: number;
  seq: number;
  outcome: Outcome;
  parent: string;
  child: string | null;
  hypothesis: string | null;
  parent_median: number | null;
  child_median: number | null;
  gain_pp: number | null;
  current_after: string;
  current_median_after: number | null;
}

export interface ExportConfig {
  repeats: number;
  accept_threshold_pp: number;
  min_paired_wins: number;
  max_tokens_per_run: number;
  max_change_bytes: number;
}

export interface Summary {
  nodes: number;
  edges: number;
  accepted: number;
  rejected: number;
  noise: number;
  pending: number;
  refused: number;
  runs: number;
  tokens_used: number;
  max_generation: number;
}

export interface LineageExport {
  format: 'rules-evolve-export';
  version: 1;
  generator: { tool: string; version: string };
  project: { rules_file: string; model: string };
  chain: { valid: boolean; events: number; head_seq: number; head_hash: string };
  config: ExportConfig;
  root: string;
  current: string;
  summary: Summary;
  nodes: TreeNode[];
  edges: TreeEdge[];
  refused: Refusal[];
  series: SeriesPoint[];
}

/// What is selected in the graph.
export type Selection =
  | { kind: 'node'; hash: string }
  | { kind: 'refused'; seq: number };
