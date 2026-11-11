<script lang="ts">
  import { onMount } from 'svelte';
  import cytoscape from 'cytoscape';
  import dagre from 'cytoscape-dagre';
  import { buildElements } from '../lib/graph';
  import { initialView, MIN_READABLE_ZOOM, NODE_FONT, pickOrientation, type Box, type Orientation } from '../lib/layout';
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
  /// Draw refused mutations as nodes. Off: they are a count on the parent node.
  let showRefused = $state(false);
  /// The view is zoomed to a readable size instead of showing the whole tree.
  let zoomedIn = $state(false);
  let hintOpen = $state(true);
  let orientation = $state<Orientation>('TB');
  /// Once the user pans or zooms, a resize does not reset the view.
  let userMoved = false;

  const untrackedShow = () => showRefused;
  const refusedCount = $derived(data.refused.length);
  const STATUSES = ['accepted', 'rejected', 'noise', 'pending', 'root', 'refused'] as const;

  function styles(): cytoscape.StylesheetJson {
    const fg = cssVar('--fg');
    const muted = cssVar('--muted');
    const list: cytoscape.StylesheetJson = [
      {
        selector: 'node',
        style: {
          shape: 'round-rectangle',
          width: 132,
          height: 'data(h)',
          label: 'data(label)',
          'text-wrap': 'wrap',
          'text-valign': 'center',
          'text-halign': 'center',
          'line-height': 1.25,
          'font-size': NODE_FONT,
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

  function layoutOptions(dir: Orientation) {
    return {
      name: 'dagre',
      animate: false,
      fit: false,
      rankDir: dir,
      // Nodes carry their own gain and refusal count, so there are no edge labels to leave room for.
      nodeSep: dir === 'TB' ? 24 : 14,
      rankSep: dir === 'TB' ? 54 : 56,
      edgeSep: 10,
      padding: 24,
    } as cytoscape.LayoutOptions;
  }

  function viewBox(): Box {
    return { w: container.clientWidth, h: container.clientHeight };
  }

  function boxOf(dir: Orientation): Box {
    cy!.layout(layoutOptions(dir)).run();
    const bb = cy!.elements().boundingBox({});
    return { w: bb.w, h: bb.h };
  }

  function focusNode(): cytoscape.NodeSingular | undefined {
    if (!cy) return undefined;
    let id: string = data.current;
    if (selection?.kind === 'node') id = selection.hash;
    else if (selection?.kind === 'refused') {
      const r = data.refused.find((x) => x.seq === selection.seq);
      const own = cy.getElementById(`refused-${selection.seq}`);
      if (own.nonempty()) return own as cytoscape.NodeSingular;
      if (r) id = r.parent;
    }
    const n = cy.getElementById(id);
    return n.nonempty() ? (n as cytoscape.NodeSingular) : undefined;
  }

  /// Center on the focus with its parent and siblings when they fit in the view, so there is
  /// context around it. Otherwise center on the focus alone.
  function centerOnFocus(view: Box) {
    const n = focusNode();
    if (!cy || !n) {
      cy?.center();
      return;
    }
    const parent = n.incomers('node');
    const group = n.union(parent).union(parent.outgoers('node'));
    const bb = group.renderedBoundingBox({});
    if (parent.nonempty() && bb.w <= view.w * 0.9 && bb.h <= view.h * 0.9) cy.center(group);
    else cy.center(n);
  }

  /// Lay the tree out (top to bottom or left to right, whichever fits bigger) and set the
  /// first view: all of it if the text stays readable, else a readable zoom around the focus.
  function arrange() {
    if (!cy || !container) return;
    cy.resize();
    const view = viewBox();
    if (view.w === 0 || view.h === 0) return;
    const tb = boxOf('TB');
    const lr = boxOf('LR');
    orientation = pickOrientation(view, tb, lr);
    cy.layout(layoutOptions(orientation)).run();
    const content = orientation === 'TB' ? tb : lr;
    const v = initialView(view, content);
    zoomedIn = !v.fitted;
    cy.zoom(v.zoom);
    if (v.fitted) {
      cy.center();
    } else {
      centerOnFocus(view);
    }
    userMoved = false;
  }

  function fitAll() {
    if (!cy) return;
    cy.fit(undefined, 24);
    if (cy.zoom() > 1.15) {
      cy.zoom(1.15);
      cy.center();
    }
    zoomedIn = false;
    userMoved = true;
  }

  function showCurrent() {
    if (!cy) return;
    const n = focusNode();
    if (!n) return;
    if (cy.zoom() < MIN_READABLE_ZOOM) cy.zoom(MIN_READABLE_ZOOM);
    cy.animate({ center: { eles: n }, duration: 200 });
    zoomedIn = cy.zoom() >= MIN_READABLE_ZOOM && zoomedIn;
    userMoved = true;
  }

  function zoomBy(factor: number) {
    if (!cy) return;
    const next = Math.min(2.5, Math.max(0.25, cy.zoom() * factor));
    cy.zoom({ level: next, renderedPosition: { x: container.clientWidth / 2, y: container.clientHeight / 2 } });
    userMoved = true;
    zoomedIn = true;
  }

  function build() {
    cy?.destroy();
    cy = cytoscape({
      container,
      elements: buildElements(data, { showRefused }),
      style: styles(),
      layout: { name: 'null' },
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
    lastShow = showRefused;
    arrange();
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

  /// Bring the selected node into view when it is off screen (a deep link, or a link in the details).
  function ensureVisible() {
    if (!cy || !selection) return;
    const id = selection.kind === 'node' ? selection.hash : `refused-${selection.seq}`;
    const n = cy.getElementById(id);
    if (n.empty()) return;
    const bb = n.renderedBoundingBox({});
    const w = container.clientWidth;
    const h = container.clientHeight;
    if (bb.x1 < 0 || bb.y1 < 0 || bb.x2 > w || bb.y2 > h) cy.animate({ center: { eles: n }, duration: 200 });
  }

  onMount(() => {
    build();
    const ro = new ResizeObserver(() => {
      if (!cy) return;
      if (userMoved) cy.resize();
      else arrange();
    });
    ro.observe(container);
    const moved = () => {
      userMoved = true;
      hintOpen = false;
    };
    container.addEventListener('wheel', moved, { passive: true });
    container.addEventListener('pointerdown', moved);
    return () => {
      ro.disconnect();
      container.removeEventListener('wheel', moved);
      container.removeEventListener('pointerdown', moved);
      cy?.destroy();
    };
  });

  // Rebuild when a different file is loaded or refused nodes are shown or hidden, restyle when the theme changes.
  let lastData: LineageExport | undefined;
  let lastTheme: Theme | undefined;
  let lastShow = false;
  $effect(() => {
    const d = data;
    const t = theme;
    const s = showRefused;
    if (!cy) return;
    if (d !== lastData || s !== lastShow) {
      build();
    } else if (t !== lastTheme) {
      cy.style(styles());
    }
    lastData = d;
    lastTheme = t;
    lastShow = s;
  });

  let lastSelected: Selection | null = null;
  $effect(() => {
    // A refused mutation chosen in the details has to be drawn to be shown. Only when the
    // selection changes, so "Hide refused" still works while one is selected.
    const sel = selection;
    if (sel !== lastSelected && sel?.kind === 'refused' && !untrackedShow()) showRefused = true;
    lastSelected = sel;
  });

  $effect(() => {
    // Track the selection and re-mark.
    void selection;
    markSelection();
    ensureVisible();
  });
</script>

<div class="wrap">
  <div class="graph" bind:this={container} role="img" aria-label="Lineage tree of rules versions"></div>
  <div class="controls" role="group" aria-label="Graph view">
    {#if refusedCount > 0}
      <button class="btn small" aria-pressed={showRefused} onclick={() => (showRefused = !showRefused)}>
        {showRefused ? 'Hide' : 'Show'} refused ({refusedCount})
      </button>
    {/if}
    <button class="btn small" onclick={showCurrent}>Current</button>
    <button class="btn small" onclick={fitAll}>Fit all</button>
    <button class="btn small" aria-label="Zoom out" onclick={() => zoomBy(1 / 1.25)}>&minus;</button>
    <button class="btn small" aria-label="Zoom in" onclick={() => zoomBy(1.25)}>+</button>
  </div>
  {#if zoomedIn && hintOpen}
    <p class="hint">Zoomed in so the text stays readable. Drag to move, scroll to zoom, or press Fit all to see the whole tree.</p>
  {/if}
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
  .controls {
    position: absolute;
    top: 10px;
    right: 10px;
    display: flex;
    gap: 0.35rem;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .hint {
    position: absolute;
    bottom: 52px;
    left: 12px;
    right: 12px;
    width: fit-content;
    max-width: calc(100% - 24px);
    margin: 0;
    padding: 0.3rem 0.7rem;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 10px;
    font-size: 0.78rem;
    color: var(--muted);
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
