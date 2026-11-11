/// Choices about how the lineage graph is laid out and how far it is zoomed.
/// Pure functions, so the rules can be tested without a browser.

/// Font size of node labels in the graph, in CSS pixels at zoom 1.
export const NODE_FONT = 13;
/// Smallest effective node label size (font size times zoom) the initial view accepts.
export const MIN_READABLE_FONT = 12;
export const MIN_READABLE_ZOOM = MIN_READABLE_FONT / NODE_FONT;
/// A small tree is not blown up past this zoom.
export const MAX_INITIAL_ZOOM = 1.15;

export interface Box {
  w: number;
  h: number;
}

export type Orientation = 'TB' | 'LR';

/// The zoom at which `content` fits in `view` with `pad` pixels around it.
export function fitZoom(view: Box, content: Box, pad = 24): number {
  const w = Math.max(1, content.w);
  const h = Math.max(1, content.h);
  const z = Math.min((view.w - 2 * pad) / w, (view.h - 2 * pad) / h);
  return Math.max(0.01, z);
}

/// Top to bottom unless left to right fits noticeably bigger (by more than 8 percent).
export function pickOrientation(view: Box, topDown: Box, leftRight: Box): Orientation {
  return fitZoom(view, leftRight) > fitZoom(view, topDown) * 1.08 ? 'LR' : 'TB';
}

export interface InitialView {
  zoom: number;
  /// The whole tree is in view. When false the view is zoomed to a readable size and the user pans.
  fitted: boolean;
}

/// Fit the whole tree if text stays readable, else zoom to a readable size.
export function initialView(view: Box, content: Box, pad = 24): InitialView {
  const z = Math.min(fitZoom(view, content, pad), MAX_INITIAL_ZOOM);
  return z >= MIN_READABLE_ZOOM ? { zoom: z, fitted: true } : { zoom: MIN_READABLE_ZOOM, fitted: false };
}

/// Effective size on screen of text drawn at `px` in the graph, at a zoom.
export function effectiveFont(zoom: number, px = NODE_FONT): number {
  return zoom * px;
}

/// Node box height for the number of label lines.
export function nodeHeight(lines: number): number {
  return 14 + 17 * Math.max(1, lines);
}
