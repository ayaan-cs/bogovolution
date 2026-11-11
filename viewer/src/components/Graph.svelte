<script lang="ts">
  import { onMount } from 'svelte';
  import cytoscape from 'cytoscape';
  import dagre from 'cytoscape-dagre';
  import { buildElements } from '../lib/graph';
  import { cssVar, type Theme } from '../lib/theme';
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

  cytoscape.use(dagre);

  let container: HTMLDivElement;
  let cy: cytoscape.Core | undefined;

  const STATUSES = ['accepted', 'rejected', 'noise', 'pending', 'root', 'refused'] as const;

  function styles(): cytoscape.StylesheetJson {
    const fg = cssVar('--fg');
    const muted = cssVar('--muted');
    const panel = cssVar('--panel');
    const list: cytoscape.StylesheetJson = [
      {
        selector: 'node',
        style: {
          shape: 'round-rectangle',
          width: 120,
          height: 46,
          label: 'data(label)',
          'text-wrap': 'wrap',
          'text-valign': 'center',
          'text-halign': 'center',
          'font-size': 13,
          'font-family': cssVar('--sans'),
          color: fg,
          'border-width': 2,
          'background-color': cssVar('--bg-pending'),
          'border-color': cssVar('--c-pending'),
        },
      },
      {
        selector: 'node[kind = "refused"]',
        style: { width: 74, height: 26, 'font-size': 11, 'border-style': 'dashed', color: muted },
      },
      {
        selector: 'node[current = 1]',
        style: { 'border-width': 4, 'underlay-color': cssVar('--c-accepted'), 'underlay-opacity': 0.18, 'underlay-padding': 6 },
      },
      {
        selector: 'node.sel',
        style: { 'border-width': 4, 'underlay-color': cssVar('--accent'), 'underlay-opacity': 0.3, 'underlay-padding': 7 },
      },
      {
        selector: 'edge',
        style: {
          width: 2,
          'curve-style': 'bezier',
          'target-arrow-shape': 'triangle',
          'arrow-scale': 1,
          label: 'data(label)',
          'font-size': 12,
          'font-family': cssVar('--sans'),
          color: muted,
          'text-background-color': panel,
          'text-background-opacity': 1,
          'text-background-padding': '3px',
          'text-background-shape': 'roundrectangle',
          'text-rotation': 'none',
          'text-wrap': 'wrap',
          'text-max-width': '100px',
          'line-color': cssVar('--c-pending'),
          'target-arrow-color': cssVar('--c-pending'),
        },
      },
      { selector: 'edge[status = "refused"]', style: { 'line-style': 'dashed', width: 1.5 } },
      { selector: 'edge.sel', style: { width: 3.5 } },
    ];
    for (const s of STATUSES) {
      list.push({
        selector: `node[status = "${s}"]`,
        style: { 'background-color': cssVar(`--bg-${s}`), 'border-color': cssVar(`--c-${s}`) },
      });
      if (s !== 'root') {
        list.push({
          selector: `edge[status = "${s}"]`,
          style: { 'line-color': cssVar(`--c-${s}`), 'target-arrow-color': cssVar(`--c-${s}`) },
        });
      }
    }
    return list;
  }

  function build() {
    cy?.destroy();
    cy = cytoscape({
      container,
      elements: buildElements(data),
      style: styles(),
      layout: {
        name: 'dagre',
        // @ts-expect-error dagre options are not in the cytoscape typings
        rankDir: 'TB',
        nodeSep: 70,
        rankSep: 76,
        edgeSep: 12,
        padding: 24,
      },
      minZoom: 0.25,
      maxZoom: 2.5,
      wheelSensitivity: 0.25,
      boxSelectionEnabled: false,
      autounselectify: true,
    });
    cy.on('tap', 'node', (evt) => {
      const id = evt.target.id() as string;
      if (id.startsWith('refused-')) onselect({ kind: 'refused', seq: Number(id.slice(8)) });
      else onselect({ kind: 'node', hash: id });
    });
    cy.on('tap', 'edge', (evt) => {
      const t = evt.target.data('target') as string;
      if (t.startsWith('refused-')) onselect({ kind: 'refused', seq: Number(t.slice(8)) });
      else onselect({ kind: 'node', hash: t });
    });
    cy.on('mouseover', 'node, edge', () => (container.style.cursor = 'pointer'));
    cy.on('mouseout', 'node, edge', () => (container.style.cursor = 'default'));
    lastData = data;
    lastTheme = theme;
    markSelection();
  }

  function markSelection() {
    if (!cy) return;
    cy.elements().removeClass('sel');
    if (!selection) return;
    const id = selection.kind === 'node' ? selection.hash : `refused-${selection.seq}`;
    const n = cy.getElementById(id);
    n.addClass('sel');
    n.incomers('edge').addClass('sel');
  }

  onMount(() => {
    build();
    const ro = new ResizeObserver(() => {
      cy?.resize();
      cy?.fit(undefined, 24);
    });
    ro.observe(container);
    return () => {
      ro.disconnect();
      cy?.destroy();
    };
  });

  // Rebuild when a different file is loaded, restyle when the theme changes.
  let lastData: LineageExport | undefined;
  let lastTheme: Theme | undefined;
  $effect(() => {
    const d = data;
    const t = theme;
    if (!cy) return;
    if (d !== lastData) {
      build();
    } else if (t !== lastTheme) {
      cy.style(styles());
    }
    lastData = d;
    lastTheme = t;
  });

  $effect(() => {
    // Track the selection and re-mark.
    void selection;
    markSelection();
  });
</script>

<div class="wrap">
  <div class="graph" bind:this={container} role="img" aria-label="Lineage tree of rules versions"></div>
  <div class="legend" aria-hidden="true">
    {#each ['accepted', 'rejected', 'noise', 'pending', 'refused'] as s}
      <span class="item"><i class="dot {s}"></i>{s}</span>
    {/each}
    <span class="item"><i class="dot ring"></i>current</span>
  </div>
</div>

<style>
  .wrap {
    position: relative;
    height: 100%;
    min-height: 460px;
  }
  .graph {
    position: absolute;
    inset: 0;
  }
  .legend {
    position: absolute;
    left: 12px;
    bottom: 10px;
    display: flex;
    gap: 0.9rem;
    flex-wrap: wrap;
    padding: 0.3rem 0.7rem;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 999px;
    font-size: 0.78rem;
    color: var(--muted);
  }
  .item {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 3px;
    border: 2px solid;
    display: inline-block;
  }
  .dot.accepted { border-color: var(--c-accepted); background: var(--bg-accepted); }
  .dot.rejected { border-color: var(--c-rejected); background: var(--bg-rejected); }
  .dot.noise { border-color: var(--c-noise); background: var(--bg-noise); }
  .dot.pending { border-color: var(--c-pending); background: var(--bg-pending); }
  .dot.refused { border-color: var(--c-refused); background: var(--bg-refused); border-style: dashed; }
  .dot.ring { border-color: var(--c-accepted); box-shadow: 0 0 0 3px color-mix(in srgb, var(--c-accepted) 25%, transparent); }
</style>
