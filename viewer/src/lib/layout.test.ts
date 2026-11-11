import cytoscape from 'cytoscape';
import dagre from 'cytoscape-dagre';
import { describe, expect, it } from 'vitest';
import demo from '../demo/tree.json';
import wide from '../../test-fixtures/wide-tree.json';
import { buildElements } from './graph';
import {
  effectiveFont,
  fitZoom,
  initialView,
  MIN_READABLE_FONT,
  MIN_READABLE_ZOOM,
  NODE_FONT,
  nodeHeight,
  pickOrientation,
  type Box,
  type Orientation,
} from './layout';
import type { LineageExport } from './types';
import { parseExport } from './validate';

cytoscape.use(dagre);

const demoData = demo as unknown as LineageExport;
const wideData = wide as unknown as LineageExport;
// The graph card at 1280x800 is about 765 by 718 pixels.
const VIEW: Box = { w: 765, h: 718 };

function layoutBox(d: LineageExport, dir: Orientation, showRefused = false): Box {
  const cy = cytoscape({
    headless: true,
    elements: buildElements(d, { showRefused }),
    style: [
      { selector: 'node', style: { width: 132, height: 'data(h)' } },
      { selector: 'node[kind = "refused"]', style: { width: 74, height: 26 } },
    ],
  });
  cy.layout({
    name: 'dagre',
    animate: false,
    rankDir: dir,
    nodeSep: dir === 'TB' ? 24 : 14,
    rankSep: dir === 'TB' ? 54 : 56,
    edgeSep: 10,
    padding: 24,
  } as cytoscape.LayoutOptions).run();
  const bb = cy.elements().boundingBox({});
  cy.destroy();
  return { w: bb.w, h: bb.h };
}

function firstView(d: LineageExport, view: Box = VIEW) {
  const tb = layoutBox(d, 'TB');
  const lr = layoutBox(d, 'LR');
  const dir = pickOrientation(view, tb, lr);
  return { dir, ...initialView(view, dir === 'TB' ? tb : lr) };
}

describe('layout helpers', () => {
  it('fit zoom is the smaller of the two axes, with padding', () => {
    expect(fitZoom({ w: 1000, h: 500 }, { w: 952, h: 100 }, 24)).toBeCloseTo(1, 5);
    expect(fitZoom({ w: 1000, h: 500 }, { w: 100, h: 904 }, 24)).toBeCloseTo(0.5, 5);
    expect(fitZoom({ w: 10, h: 10 }, { w: 500, h: 500 })).toBeGreaterThan(0);
  });

  it('picks left to right only when it clearly fits bigger', () => {
    expect(pickOrientation(VIEW, { w: 700, h: 400 }, { w: 400, h: 700 })).toBe('TB');
    expect(pickOrientation(VIEW, { w: 1600, h: 400 }, { w: 400, h: 900 })).toBe('LR');
    // Within 8 percent keeps the familiar top to bottom.
    expect(pickOrientation(VIEW, { w: 1000, h: 500 }, { w: 960, h: 500 })).toBe('TB');
  });

  it('fits when readable and otherwise zooms to a readable size', () => {
    const small = initialView(VIEW, { w: 500, h: 400 });
    expect(small.fitted).toBe(true);
    expect(small.zoom).toBeGreaterThanOrEqual(MIN_READABLE_ZOOM);
    const huge = initialView(VIEW, { w: 4000, h: 3000 });
    expect(huge.fitted).toBe(false);
    expect(huge.zoom).toBe(MIN_READABLE_ZOOM);
    // A tiny tree is not blown up.
    expect(initialView(VIEW, { w: 100, h: 50 }).zoom).toBeLessThanOrEqual(1.15);
  });

  it('readable zoom means the label font is at least the target', () => {
    expect(effectiveFont(MIN_READABLE_ZOOM)).toBeCloseTo(MIN_READABLE_FONT, 5);
    expect(MIN_READABLE_FONT).toBeGreaterThanOrEqual(11);
  });

  it('node height grows with lines', () => {
    expect(nodeHeight(2)).toBeLessThan(nodeHeight(3));
    expect(nodeHeight(0)).toBe(nodeHeight(1));
  });
});

describe('the demo at 1280x800', () => {
  it('fits whole at a readable size', () => {
    const v = firstView(demoData);
    expect(v.fitted).toBe(true);
    expect(effectiveFont(v.zoom)).toBeGreaterThanOrEqual(MIN_READABLE_FONT);
    expect(NODE_FONT).toBe(13);
  });
});

describe('a wide synthetic tree', () => {
  it('is a valid export with 30 or more versions and 8 children on one parent', () => {
    expect(parseExport(JSON.stringify(wideData)).ok).toBe(true);
    expect(wideData.nodes.length).toBeGreaterThanOrEqual(30);
    const counts = new Map<string, number>();
    for (const n of wideData.nodes) if (n.parent) counts.set(n.parent, (counts.get(n.parent) ?? 0) + 1);
    expect(Math.max(...counts.values())).toBeGreaterThanOrEqual(8);
  });

  it('keeps the label text readable in the first view', () => {
    const v = firstView(wideData);
    expect(effectiveFont(v.zoom)).toBeGreaterThanOrEqual(MIN_READABLE_FONT);
  });

  it('drawing refused nodes as well stays readable', () => {
    const tb = layoutBox(wideData, 'TB', true);
    const lr = layoutBox(wideData, 'LR', true);
    const dir = pickOrientation(VIEW, tb, lr);
    const v = initialView(VIEW, dir === 'TB' ? tb : lr);
    expect(effectiveFont(v.zoom)).toBeGreaterThanOrEqual(MIN_READABLE_FONT);
  });

  it('is narrower without refused nodes in the rows', () => {
    const hidden = layoutBox(wideData, 'TB');
    const shown = layoutBox(wideData, 'TB', true);
    expect(hidden.w).toBeLessThanOrEqual(shown.w);
  });
});
