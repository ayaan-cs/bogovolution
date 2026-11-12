<script lang="ts">
  import * as Plot from '@observablehq/plot';
  import { paddedAxis, percentAxis } from '../lib/chart';
  import { pct, points, shortHash } from '../lib/format';
  import { cssVar, type Theme } from '../lib/theme';
  import type { LineageExport, Selection } from '../lib/types';

  let {
    data,
    selection,
    theme,
  }: { data: LineageExport; selection: Selection | null; theme: Theme } = $props();

  let passEl: HTMLDivElement;
  let gainEl: HTMLDivElement;
  let spreadEl: HTMLDivElement;
  let passW = $state(0);
  let gainW = $state(0);
  let spreadW = $state(0);

  const OUTCOMES = ['accepted', 'rejected', 'noise', 'overfit', 'pending', 'refused'];

  function palette() {
    return {
      domain: OUTCOMES,
      range: OUTCOMES.map((o) => cssVar(`--c-${o}`)),
    };
  }

  function base(width: number, height: number) {
    return {
      width: Math.max(280, width),
      height,
      style: { background: 'transparent', color: cssVar('--fg'), fontSize: '12px', fontFamily: cssVar('--sans') },
      marginTop: 30,
      marginLeft: 44,
      marginBottom: 36,
    };
  }

  function put(el: HTMLDivElement, plot: (SVGSVGElement | HTMLElement)) {
    el.replaceChildren(plot);
  }

  // Chart 1: how the pass rate of the current version moved, attempt by attempt.
  $effect(() => {
    void theme;
    if (!passEl || data.series.length === 0) return passEl?.replaceChildren();
    const s = data.series;
    const accent = cssVar('--accent');
    put(
      passEl,
      Plot.plot({
        ...base(passW, 280),
        x: { label: 'Attempt', tickFormat: 'd', ticks: Math.min(s.length, 12), grid: false },
        y: { label: 'Median pass rate (%)', domain: percentAxis().domain, ticks: percentAxis().ticks, grid: true },
        color: palette(),
        marks: [
          Plot.lineY(s, { x: 'attempt', y: (d) => (d.current_median_after ?? NaN) * 100, curve: 'step-after', stroke: accent, strokeWidth: 2 }),
          Plot.dot(s.filter((d) => d.child_median !== null), {
            x: 'attempt',
            y: (d) => (d.child_median as number) * 100,
            fill: 'outcome',
            r: 6,
            stroke: cssVar('--panel'),
            strokeWidth: 1.5,
            tip: true,
            title: (d) => `Attempt ${d.attempt}: ${d.outcome}\n${d.hypothesis ?? ''}\nchild ${pct(d.child_median)}, parent ${pct(d.parent_median)}`,
          }),
          Plot.dot(s.filter((d) => d.outcome === 'refused'), {
            x: 'attempt',
            y: (d) => (d.current_median_after ?? 0) * 100,
            symbol: 'times',
            stroke: cssVar('--c-refused'),
            strokeWidth: 2,
            r: 6,
            tip: true,
            title: (d) => `Attempt ${d.attempt}: refused\n${d.hypothesis ?? 'no hypothesis'}`,
          }),
        ],
      }),
    );
  });

  // Chart 2: gain of each attempt against the accept threshold.
  $effect(() => {
    void theme;
    if (!gainEl || data.series.length === 0) return gainEl?.replaceChildren();
    const s = data.series.filter((d) => d.gain_pp !== null);
    const thr = data.config.accept_threshold_pp;
    // Always room for every bar and the accept line, so nothing touches or passes the edge.
    const axis = paddedAxis(s.map((d) => d.gain_pp), { include: [thr] });
    put(
      gainEl,
      Plot.plot({
        ...base(gainW, 280),
        x: { label: 'Attempt', tickFormat: 'd', ticks: Math.min(data.series.length, 12), domain: data.series.map((d) => d.attempt), type: 'band' },
        y: { label: 'Gain over parent (points)', domain: axis.domain, ticks: axis.ticks, grid: true },
        color: palette(),
        marks: [
          Plot.barY(s, {
            x: 'attempt',
            y: 'gain_pp',
            fill: 'outcome',
            tip: true,
            title: (d) => `Attempt ${d.attempt}: ${d.outcome}\n${d.hypothesis ?? ''}\n${points(d.gain_pp)}`,
          }),
          Plot.ruleY([0], { stroke: cssVar('--muted') }),
          Plot.ruleY([thr], { stroke: cssVar('--c-accepted'), strokeDasharray: '5,4' }),
          Plot.text([`accept at +${thr} pts`], { y: [thr], frameAnchor: 'right', textAnchor: 'end', dy: -8, fill: cssVar('--tx-accepted') }),
        ],
      }),
    );
  });

  // Chart 3: every run of every version, to show how noisy the benchmark is.
  $effect(() => {
    void theme;
    const sel = selection?.kind === 'node' ? selection.hash : null;
    if (!spreadEl) return;
    const nodes = data.nodes.filter((n) => n.runs.length > 0);
    if (nodes.length === 0) return spreadEl.replaceChildren();
    const label = (n: (typeof nodes)[number]) => shortHash(n.hash, 7);
    const labels = nodes.map(label);
    const runs = nodes.flatMap((n, idx) =>
      n.runs.map((r, i) => ({
        v: label(n),
        status: n.status,
        rate: r.pass_rate * 100,
        selected: n.hash === sel,
        run: r.run_index + 1,
        // Spread dots sideways a little so equal rates do not hide each other.
        x: idx + (i - (n.runs.length - 1) / 2) * 0.13,
      })),
    );
    const meds = nodes.map((n, idx) => ({ idx, rate: (n.median_pass_rate ?? 0) * 100, status: n.status }));
    const statusColor = {
      domain: ['root', 'accepted', 'rejected', 'noise', 'overfit', 'pending'],
      range: ['root', 'accepted', 'rejected', 'noise', 'overfit', 'pending'].map((o) => cssVar(`--c-${o}`)),
    };
    put(
      spreadEl,
      Plot.plot({
        ...base(spreadW, 300),
        marginBottom: 56,
        x: {
          label: null,
          domain: [-0.6, nodes.length - 0.4],
          ticks: labels.map((_, i) => i),
          tickFormat: (i: number) => labels[i] ?? '',
          tickRotate: -40,
        },
        y: { label: 'Pass rate per run (%)', domain: percentAxis().domain, ticks: percentAxis().ticks, grid: true },
        color: statusColor,
        marks: [
          Plot.dot(runs, {
            x: 'x',
            y: 'rate',
            fill: 'status',
            fillOpacity: 0.7,
            r: 4,
            stroke: (d) => (d.selected ? cssVar('--fg') : 'none'),
            strokeWidth: 1.5,
            tip: true,
            title: (d) => `${d.v}, run ${d.run}: ${Math.round(d.rate)}%`,
          }),
          Plot.ruleY(meds, { x1: (d) => d.idx - 0.36, x2: (d) => d.idx + 0.36, y: 'rate', stroke: 'status', strokeWidth: 3 }),
        ],
      }),
    );
  });
</script>

<section class="charts">
  <div class="card chart">
    <h3>Pass rate, attempt by attempt</h3>
    <p class="note">The line is the current version's median pass rate. Dots are each attempt's child. Crosses are refused attempts.</p>
    <div bind:this={passEl} bind:clientWidth={passW}></div>
  </div>
  <div class="card chart">
    <h3>Gain per attempt</h3>
    <p class="note">Change in median pass rate over the parent. Only bars past the dashed line can be accepted, and only with enough paired wins.</p>
    <div bind:this={gainEl} bind:clientWidth={gainW}></div>
  </div>
  <div class="card chart wide">
    <h3>Run spread per version</h3>
    <p class="note">Each dot is one run of the benchmark. The bar is the median. Wide spread means a small gain may just be noise.</p>
    <div bind:this={spreadEl} bind:clientWidth={spreadW}></div>
  </div>
</section>

<style>
  .charts {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 1rem;
  }
  .chart {
    padding: 0.9rem 1.1rem 0.6rem;
    min-width: 0;
  }
  .wide {
    grid-column: 1 / -1;
  }
  h3 {
    margin: 0;
    font-size: 1rem;
  }
  .note {
    margin: 0.15rem 0 0.4rem;
    font-size: 0.82rem;
    color: var(--muted);
  }
  @media (max-width: 900px) {
    .charts {
      grid-template-columns: 1fr;
    }
  }
</style>
