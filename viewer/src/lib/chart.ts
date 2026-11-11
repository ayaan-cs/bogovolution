/// Axis helpers for the charts. Pure, so they can be tested without a browser.

export interface Axis {
  /// [low, high] of the scale. Always contains every value given, with room around it.
  domain: [number, number];
  /// Tick values, evenly spaced and always inside the domain.
  ticks: number[];
  step: number;
}

/// A tidy step (1, 2 or 5 times a power of ten) that gives about `target` intervals over `span`.
export function niceStep(span: number, target = 6): number {
  if (!(span > 0) || !Number.isFinite(span)) return 1;
  const raw = span / target;
  const mag = Math.pow(10, Math.floor(Math.log10(raw)));
  const norm = raw / mag;
  const f = norm < 1.5 ? 1 : norm < 3.5 ? 2 : norm < 7.5 ? 5 : 10;
  return f * mag;
}

function clean(n: number): number {
  // Remove float noise such as 0.30000000000000004.
  return Math.round(n * 1e9) / 1e9;
}

/// A y axis that always includes zero, every value and every extra line (such as the accept
/// threshold), with padding so a bar or dot at the extreme is never touching or past the edge.
/// The domain ends on tick values, so the axis labels match what is drawn.
export function paddedAxis(values: Array<number | null | undefined>, opts: { include?: number[]; pad?: number; target?: number } = {}): Axis {
  const pts = [0, ...(opts.include ?? []), ...values].filter((v): v is number => typeof v === 'number' && Number.isFinite(v));
  let lo = Math.min(...pts);
  let hi = Math.max(...pts);
  if (hi === lo) hi = lo + 1;
  const step = niceStep(hi - lo, opts.target ?? 6);
  const pad = (hi - lo) * (opts.pad ?? 0.08);
  // Round outward to a multiple of the step, past the padded extremes.
  const low = lo < 0 ? clean(Math.floor((lo - pad) / step) * step) : 0;
  const high = hi > 0 ? clean(Math.ceil((hi + pad) / step) * step) : 0;
  const ticks: number[] = [];
  for (let t = low; t <= high + step / 1000; t += step) ticks.push(clean(t));
  return { domain: [low, high], ticks, step };
}

/// A 0 to 100 percent axis with a little room above and below, ticks every 20.
export function percentAxis(pad = 5): Axis {
  return { domain: [-pad, 100 + pad], ticks: [0, 20, 40, 60, 80, 100], step: 20 };
}
