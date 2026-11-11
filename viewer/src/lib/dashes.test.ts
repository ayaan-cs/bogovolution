import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) return files(p);
    return /\.(ts|svelte|css|html|json)$/.test(name) ? [p] : [];
  });
}

describe('house style', () => {
  it('has no em dashes or en dashes in viewer sources or UI copy', () => {
    const bad = files('src').filter((f) => /[\u2013\u2014]/.test(readFileSync(f, 'utf8')));
    expect(bad).toEqual([]);
  });
});
