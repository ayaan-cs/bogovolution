<script lang="ts">
  import DiffView from './DiffView.svelte';
  import { edgeForChild, nodeById, pct, points, shortHash, verdictSummary } from '../lib/format';
  import type { Theme } from '../lib/theme';
  import type { LineageExport, Selection } from '../lib/types';

  let {
    data,
    selection,
    theme,
    onselect,
  }: {
    data: LineageExport;
    selection: Selection | null;
    theme: Theme;
    onselect: (s: Selection) => void;
  } = $props();

  const node = $derived(selection?.kind === 'node' ? nodeById(data, selection.hash) : undefined);
  const edge = $derived(node ? edgeForChild(data, node.hash) : undefined);
  const parent = $derived(node?.parent ? nodeById(data, node.parent) : undefined);
  const refusal = $derived(selection?.kind === 'refused' ? data.refused.find((r) => r.seq === selection.seq) : undefined);
  let showContent = $state(false);
  $effect(() => {
    void selection;
    showContent = false;
  });
</script>

<aside class="details card" aria-live="polite">
  {#if node}
    <header>
      <span class="badge {node.status}">{node.status}</span>
      {#if node.is_current}<span class="badge accepted">current</span>{/if}
      <h2 class="mono">{shortHash(node.hash, 12)}</h2>
    </header>
    <p class="meta">
      Generation {node.generation} · {node.size_bytes} bytes
      {#if node.median_pass_rate !== null} · median pass rate {pct(node.median_pass_rate)}{/if}
    </p>

    {#if node.hypothesis}
      <h3>Hypothesis</h3>
      <p class="hyp">{node.hypothesis}</p>
    {:else}
      <p class="hyp muted">This is the starting rules file. Nothing was changed to make it.</p>
    {/if}

    {#if edge}
      <h3>Verdict</h3>
      <p class="reason">{verdictSummary(edge, data.config)}</p>
      <dl class="stats">
        <div><dt>Parent median</dt><dd>{pct(edge.parent_median)}</dd></div>
        <div><dt>Child median</dt><dd>{pct(edge.child_median)}</dd></div>
        <div><dt>Gain</dt><dd>{points(edge.gain_pp)}</dd></div>
        <div>
          <dt>Paired wins</dt>
          <dd>{edge.paired_wins === null ? 'n/a' : `${edge.paired_wins} of ${edge.repeats}`}</dd>
        </div>
      </dl>
      <p class="rule">
        Rule: accept only with a gain of at least {data.config.accept_threshold_pp} points and at least
        {data.config.min_paired_wins} paired wins. Otherwise reject if the child is not above the parent, else call it noise.
      </p>

      <h3>Run pairs</h3>
      {#if edge.pairs.length === 0}
        <p class="muted">No runs recorded for both versions yet.</p>
      {:else}
        <table>
          <thead><tr><th>Run</th><th>Parent</th><th>Child</th><th></th></tr></thead>
          <tbody>
            {#each edge.pairs as p}
              <tr class:win={p.win}>
                <td>{p.run_index + 1}</td>
                <td>{pct(p.parent_rate)}</td>
                <td>{pct(p.child_rate)}</td>
                <td>{p.win ? 'child wins' : p.child_rate < p.parent_rate ? 'parent wins' : 'tie'}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {/if}

    {#if node.diff}
      <h3>
        Change from
        {#if parent}
          <button class="node-link mono" onclick={() => onselect({ kind: 'node', hash: parent.hash })}>
            {shortHash(parent.hash, 7)}
          </button>
        {/if}
      </h3>
      <DiffView diff={node.diff} {theme} />
    {/if}

    <h3>
      <button class="btn small" onclick={() => (showContent = !showContent)}>
        {showContent ? 'Hide' : 'Show'} full rules text
      </button>
    </h3>
    {#if showContent}
      <pre class="content">{node.content}</pre>
    {/if}
  {:else if refusal}
    <header>
      <span class="badge refused">refused</span>
      <h2>Refused mutation</h2>
    </header>
    <p class="meta">
      Tried on
      <button class="node-link mono" onclick={() => onselect({ kind: 'node', hash: refusal.parent })}>
        {shortHash(refusal.parent, 7)}
      </button>
      · log entry {refusal.seq}
    </p>
    <h3>Hypothesis</h3>
    <p class="hyp">{refusal.hypothesis ?? 'The proposal had no hypothesis.'}</p>
    <h3>Why it was refused</h3>
    <p class="reason">{refusal.reason}</p>
    <p class="rule">Refused changes never become versions and are never run, so they cost no benchmark time.</p>
    {#if refusal.provider || refusal.seed !== null}
      <p class="muted">
        {#if refusal.provider}Provider: {refusal.provider}.{/if}
        {#if refusal.seed !== null} Seed: {refusal.seed}.{/if}
      </p>
    {/if}
  {:else}
    <p class="muted">Click a version in the tree to see what changed and why it was kept or dropped.</p>
  {/if}
</aside>

<style>
  .details {
    padding: 1rem 1.15rem 1.2rem;
    overflow: auto;
    max-height: 100%;
  }
  header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
  h2 {
    margin: 0;
    font-size: 1.1rem;
  }
  h3 {
    margin: 1.1rem 0 0.35rem;
    font-size: 0.78rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  p {
    margin: 0.3rem 0;
  }
  .meta,
  .muted {
    color: var(--muted);
  }
  .hyp {
    font-size: 1.02rem;
    font-weight: 560;
  }
  .reason {
    padding: 0.5rem 0.7rem;
    background: var(--panel-2);
    border-radius: 8px;
    border-left: 3px solid var(--accent);
  }
  .rule {
    font-size: 0.8rem;
    color: var(--muted);
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 0.5rem;
    margin: 0.6rem 0;
  }
  .stats div {
    background: var(--panel-2);
    border-radius: 8px;
    padding: 0.4rem 0.6rem;
  }
  dt {
    font-size: 0.7rem;
    color: var(--muted);
  }
  dd {
    margin: 0;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.88rem;
    font-variant-numeric: tabular-nums;
  }
  th,
  td {
    text-align: left;
    padding: 0.25rem 0.5rem;
    border-bottom: 1px solid var(--line);
  }
  th {
    color: var(--muted);
    font-weight: 600;
    font-size: 0.75rem;
  }
  tr.win td:last-child {
    color: var(--tx-accepted);
    font-weight: 600;
  }
  .node-link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    cursor: pointer;
    text-decoration: underline;
    text-transform: none;
    letter-spacing: 0;
  }
  .btn.small {
    padding: 0.25rem 0.6rem;
    font-size: 0.82rem;
    text-transform: none;
    letter-spacing: 0;
  }
  .content {
    background: var(--panel-2);
    border-radius: 8px;
    padding: 0.7rem;
    overflow: auto;
    max-height: 18rem;
    font-size: 0.82rem;
    white-space: pre-wrap;
    margin: 0;
  }
</style>
