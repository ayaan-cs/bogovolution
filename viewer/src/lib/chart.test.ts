import { describe, expect, it } from 'vitest';
import { niceStep, paddedAxis, percentAxis } from './chart';

describe('niceStep', () => {
  it('picks 1, 2 or 5 times a power of ten', () => {
    expect(niceStep(75)).toBe(10);
    expect(niceStep(33)).toBe(5);
    expect(niceStep(10)).toBe(2);
    expect(niceStep(1)).toBe(0.2);
    expect(niceStep(0)).toBe(1);
    expect(niceStep(NaN)).toBe(1);
  });
});

describe('paddedAxis', () => {
  it('leaves room above a +33.3 bar and below a -41.7 bar', () => {
    const a = paddedAxis([33.3, 33.3, -41.7, 8.3], { include: [10] });
    expect(a.domain[1]).toBeGreaterThan(33.3);
    expect(a.domain[0]).toBeLessThan(-41.7);
    expect(a.ticks[0]).toBe(a.domain[0]);
    expect(a.ticks[a.ticks.length - 1]).toBe(a.domain[1]);
    expect(a.ticks).toContain(0);
  });

  it('always includes the accept threshold, even when every bar is small', () => {
    const a = paddedAxis([1, 2, -1], { include: [10] });
    expect(a.domain[1]).toBeGreaterThan(10);
    expect(a.domain[0]).toBeLessThanOrEqual(-1);
  });

  it('includes zero when every value is positive or every value is negative', () => {
    expect(paddedAxis([20, 30]).domain[0]).toBe(0);
    expect(paddedAxis([-20, -30]).domain[1]).toBe(0);
  });

  it('copes with no data, nulls and equal values', () => {
    const empty = paddedAxis([]);
    expect(empty.domain[1]).toBeGreaterThan(empty.domain[0]);
    expect(paddedAxis([null, undefined]).domain[1]).toBeGreaterThan(0);
    const same = paddedAxis([5, 5, 5]);
    expect(same.domain[1]).toBeGreaterThan(5);
  });

  it('ticks are evenly spaced, tidy and inside the domain', () => {
    for (const vals of [[33.3, -41.7], [8.3], [100, -100], [0.4, -0.2], [1234, -50]]) {
      const a = paddedAxis(vals, { include: [10] });
      for (const t of a.ticks) {
        expect(t).toBeGreaterThanOrEqual(a.domain[0]);
        expect(t).toBeLessThanOrEqual(a.domain[1]);
      }
      for (let i = 1; i < a.ticks.length; i++) expect(a.ticks[i] - a.ticks[i - 1]).toBeCloseTo(a.step, 6);
      for (const v of vals) {
        expect(v).toBeGreaterThanOrEqual(a.domain[0]);
        expect(v).toBeLessThanOrEqual(a.domain[1]);
      }
    }
  });

  it('every value fits with some padding to spare', () => {
    const a = paddedAxis([33.3], { include: [10], pad: 0.08 });
    const span = a.domain[1] - a.domain[0];
    expect((a.domain[1] - 33.3) / span).toBeGreaterThan(0.03);
  });
});

describe('percentAxis', () => {
  it('shows 0 to 100 with room on both sides', () => {
    const a = percentAxis();
    expect(a.domain[0]).toBeLessThan(0);
    expect(a.domain[1]).toBeGreaterThan(100);
    expect(a.ticks).toEqual([0, 20, 40, 60, 80, 100]);
  });
});
