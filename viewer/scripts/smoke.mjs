// Browser smoke test of the built viewer (needs an installed Chrome).
//   npm run build && npm run smoke
// Checks: the demo draws, a bad file shows a friendly error and keeps the old
// lineage, a good file loads through the file picker, and nothing touches the network.
import { chromium } from 'playwright-core';
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const dist = join(root, 'dist', 'index.html');
const executablePath = [process.env.CHROME_PATH, '/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser']
  .filter(Boolean)
  .find((p) => existsSync(p));
assert.ok(executablePath, 'No Chrome found. Set CHROME_PATH.');
assert.ok(existsSync(dist), 'Run npm run build first.');

const tmp = mkdtempSync(join(tmpdir(), 're-viewer-'));
const bad = join(tmp, 'bad.json');
writeFileSync(bad, '{"hello": "world"}');
const demo = JSON.parse(readFileSync(join(root, 'src', 'demo', 'tree.json'), 'utf8'));
demo.project.model = 'smoke-test-model';
const good = join(tmp, 'good.json');
writeFileSync(good, JSON.stringify(demo));

const browser = await chromium.launch({ executablePath, args: ['--no-sandbox'] });
const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
const outside = [];
const errors = [];
page.on('request', (r) => !/^(file|data|blob):/.test(r.url()) && outside.push(r.url()));
page.on('pageerror', (e) => errors.push(e.message));

await page.goto(pathToFileURL(dist).href);
await page.waitForSelector('canvas');
assert.match(await page.textContent('h1'), /rules-evolve lineage/);
assert.match(await page.textContent('.sub'), /demo-model/);
assert.match(await page.textContent('footer'), /Petri/);

// Bad file: friendly error, old lineage stays.
await page.setInputFiles('input[type=file]', bad);
await page.waitForSelector('[role=alert]');
assert.match(await page.textContent('[role=alert]'), /does not look like a rules-evolve export/);
assert.match(await page.textContent('.sub'), /demo-model/);

// Good file replaces it and clears the error.
await page.setInputFiles('input[type=file]', good);
await page.waitForFunction(() => document.querySelector('.sub')?.textContent?.includes('smoke-test-model'));
assert.equal(await page.locator('[role=alert]').count(), 0);

// Theme toggle changes the document theme.
const before = await page.evaluate(() => document.documentElement.dataset.theme);
await page.click('text=/(Dark|Light) theme/');
const after = await page.evaluate(() => document.documentElement.dataset.theme);
assert.notEqual(before, after);

// Drag and drop onto the page loads a file too.
const dropped = JSON.stringify({ ...demo, project: { ...demo.project, model: 'dropped-model' } });
await page.evaluate((text) => {
  const dt = new DataTransfer();
  dt.items.add(new File([text], 'dropped.json', { type: 'application/json' }));
  window.dispatchEvent(new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true }));
}, dropped);
await page.waitForFunction(() => document.querySelector('.sub')?.textContent?.includes('dropped-model'));

// A ?tree= URL that cannot be fetched from disk gives a friendly error.
await page.goto(pathToFileURL(dist).href + '?tree=./nope.json');
await page.waitForSelector('[role=alert]');
assert.match(await page.textContent('[role=alert]'), /Could not load that URL/);

await browser.close();
assert.deepEqual(errors, [], 'page errors: ' + errors.join('; '));
assert.deepEqual(outside.filter((u) => !u.startsWith('file:')), [], 'network requests: ' + outside.join(', '));
console.log('smoke ok');
