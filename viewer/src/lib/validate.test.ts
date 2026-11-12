import { describe, expect, it } from 'vitest';
import demo from '../demo/tree.json';
import { checkReferences, parseExport, prettyPath } from './validate';
import type { LineageExport } from './types';

const demoText = JSON.stringify(demo);
const clone = (): any => JSON.parse(demoText);

function failure(text: string) {
  const r = parseExport(text);
  if (r.ok) throw new Error('expected a failure');
  return r;
}

describe('parseExport', () => {
  it('accepts the bundled demo export', () => {
    const r = parseExport(demoText);
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(r.data.nodes).toHaveLength(9);
      expect(r.data.edges).toHaveLength(8);
      expect(r.data.refused).toHaveLength(2);
      expect(r.data.series).toHaveLength(10);
      expect(r.data.format).toBe('rules-evolve-export');
    }
  });

  it('accepts held out fields and the overfit verdict', () => {
    const d = clone();
    d.config.held_out_accept_threshold_pp = 10;
    d.config.held_out_min_paired_wins = 3;
    d.summary.overfit = 1;
    d.edges[0].verdict = 'overfit';
    d.edges[0].held_out_parent_median = 0.4;
    d.edges[0].held_out_child_median = 0.4;
    d.edges[0].held_out_gain_pp = 0;
    d.edges[0].held_out_paired_wins = 0;
    const child = d.nodes.find((n: any) => n.hash === d.edges[0].child);
    child.verdict = 'overfit';
    child.status = 'overfit';
    child.held_out_median_pass_rate = 0.4;
    child.runs[0].held_out_passed = 2;
    child.runs[0].held_out_total = 5;
    child.runs[0].held_out_pass_rate = 0.4;
    d.series[0].outcome = 'overfit';
    const r = parseExport(JSON.stringify(d));
    expect(r.ok).toBe(true);
    // a held out rate above 1 is still caught
    child.runs[0].held_out_pass_rate = 1.5;
    expect(parseExport(JSON.stringify(d)).ok).toBe(false);
  });

  it('explains an empty file', () => {
    expect(failure('   ').title).toMatch(/empty/i);
  });

  it('explains text that is not JSON', () => {
    const r = failure('{"format": ');
    expect(r.title).toMatch(/not valid JSON/);
    expect(r.details.length).toBe(1);
  });

  it('explains JSON that is not an object', () => {
    expect(failure('[1,2,3]').title).toMatch(/not a lineage export/);
    expect(failure('42').title).toMatch(/not a lineage export/);
    expect(failure('null').title).toMatch(/not a lineage export/);
  });

  it('explains a different kind of JSON file', () => {
    const r = failure('{"name": "package.json"}');
    expect(r.title).toMatch(/does not look like a rules-evolve export/);
    expect(r.message).toContain('rules-evolve export --out tree.json');
  });

  it('explains a newer export version', () => {
    const d = clone();
    d.version = 2;
    const r = failure(JSON.stringify(d));
    expect(r.title).toMatch(/newer/);
    expect(r.message).toContain('version 1');
  });

  it('explains an unknown version', () => {
    const d = clone();
    d.version = 'one';
    expect(failure(JSON.stringify(d)).title).toMatch(/Unsupported/);
  });

  it('names the missing field', () => {
    const d = clone();
    delete d.nodes[2].hypothesis;
    const r = failure(JSON.stringify(d));
    expect(r.title).toMatch(/schema/);
    expect(r.details[0]).toBe('nodes[2] is missing the field "hypothesis"');
  });

  it('names an unexpected field', () => {
    const d = clone();
    d.config.extra = 1;
    expect(failure(JSON.stringify(d)).details[0]).toBe('config has an unexpected field "extra"');
  });

  it('names a wrong type and a bad value', () => {
    const d = clone();
    d.summary.nodes = 'nine';
    expect(failure(JSON.stringify(d)).details[0]).toMatch(/^summary\.nodes should be of type integer/);
    const e = clone();
    e.edges[0].verdict = 'great';
    expect(failure(JSON.stringify(e)).details.length).toBeGreaterThan(0);
  });

  it('rejects a bad hash', () => {
    const d = clone();
    d.root = 'abc';
    expect(failure(JSON.stringify(d)).details[0]).toMatch(/^root /);
  });

  it('catches references to versions that are not there', () => {
    const d = clone();
    d.edges[0].child = 'f'.repeat(64);
    const r = failure(JSON.stringify(d));
    expect(r.title).toMatch(/inconsistent/);
    expect(r.details[0]).toContain('edges[0]');
  });

  it('never throws on odd input', () => {
    for (const text of ['', '{', '{}', '[]', '"x"', '\u0000', '{"format":"rules-evolve-export","version":1}']) {
      expect(() => parseExport(text)).not.toThrow();
      expect(parseExport(text).ok).toBe(false);
    }
  });
});

describe('prettyPath', () => {
  it('turns pointers into readable paths', () => {
    expect(prettyPath('')).toBe('the file');
    expect(prettyPath('/nodes/3/hash')).toBe('nodes[3].hash');
    expect(prettyPath('/config/repeats')).toBe('config.repeats');
    expect(prettyPath('/series/0')).toBe('series[0]');
  });
});

describe('checkReferences', () => {
  it('finds nothing wrong with the demo', () => {
    expect(checkReferences(clone() as LineageExport)).toEqual([]);
  });

  it('finds duplicate versions and a missing root', () => {
    const d = clone();
    d.nodes.push({ ...d.nodes[1] });
    d.root = 'a'.repeat(64);
    const problems = checkReferences(d);
    expect(problems.some((p) => p.includes('appears twice'))).toBe(true);
    expect(problems.some((p) => p.includes('root version'))).toBe(true);
  });
});
