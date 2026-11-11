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

// Graph readability at the initial fit: node text is at least 11.5 CSS pixels.
const graphStats = () =>
  page.evaluate(() => {
    const c = document.querySelector('.graph');
    const cy = c._cyreg.cy;
    return { font: cy.zoom() * 13, nodes: cy.nodes().length, w: c.clientWidth, h: c.clientHeight };
  });
{
  const g = await page.evaluate(() => { const cy = document.querySelector('.graph')._cyreg.cy; return { font: cy.zoom() * 13, nodes: cy.nodes().length }; });
  assert.ok(g.font >= 11.5, `demo node text is ${g.font.toFixed(1)}px, wanted at least 11.5`);
  assert.equal(g.nodes, 9, 'refused mutations are a count on the parent, not nodes');
  assert.equal(await page.locator('.hint').count(), 0, 'the whole demo fits, so no zoom hint');
  assert.match(await page.textContent('.legend'), /refused/);
}

// The refusal reasons stay reachable: the version lists them, and opening one draws it.
{
  await page.click('.refused-list button');
  await page.waitForFunction(() => document.querySelector('.details')?.textContent?.includes('Refused mutation'));
  const n = await page.evaluate(() => document.querySelector('.graph')._cyreg.cy.nodes().length);
  assert.equal(n, 11, 'selecting a refusal shows the refused nodes');
  await page.click('text=/Hide refused/');
  assert.equal(await page.evaluate(() => document.querySelector('.graph')._cyreg.cy.nodes().length), 9);
}

// A deep link to a node puts it in view and selects it.
{
  await page.goto(pathToFileURL(dist).href + '#v=ec12bc56');
  await page.waitForSelector('canvas');
  await page.waitForFunction(() => document.querySelector('.details')?.textContent?.includes('ec12bc56d472'));
  const inView = await page.evaluate(() => {
    const c = document.querySelector('.graph');
    const bb = c._cyreg.cy.getElementById('ec12bc56d472c81df96086bf6f174bb6ff58ccba2f23228af68e9db7645f8176').renderedBoundingBox({});
    return bb.x1 >= 0 && bb.y1 >= 0 && bb.x2 <= c.clientWidth && bb.y2 <= c.clientHeight;
  });
  assert.ok(inView, 'the deep linked node is inside the graph view');
  await page.goto(pathToFileURL(dist).href);
  await page.waitForSelector('canvas');
}

// A wide synthetic tree (34 versions, 8 children on one parent) stays readable and can be panned and fitted.
{
  const wide = join(root, 'test-fixtures', 'wide-tree.json');
  await page.setInputFiles('input[type=file]', wide);
  await page.waitForFunction(() => document.querySelector('.sub')?.textContent?.includes('wide-fixture'));
  await page.waitForTimeout(500);
  const g = await graphStats();
  assert.ok(g.font >= 11.5, `wide tree node text is ${g.font.toFixed(1)}px, wanted at least 11.5`);
  assert.equal(g.nodes, 34);
  assert.equal(await page.locator('.hint').count(), 1, 'zoomed in, so the hint explains how to move');
  const current = await page.evaluate(() => {
    const c = document.querySelector('.graph');
    const cy = c._cyreg.cy;
    const bb = cy.nodes('[current = 1]')[0].renderedBoundingBox({});
    return bb.x1 >= 0 && bb.y1 >= 0 && bb.x2 <= c.clientWidth && bb.y2 <= c.clientHeight;
  });
  assert.ok(current, 'the first view is centered on the current version');
  await page.click('text=/Show refused/');
  assert.equal((await graphStats()).nodes, 40);
  await page.click('text=Fit all');
  assert.equal(await page.locator('.hint').count(), 0);
  const fitted = await page.evaluate(() => {
    const c = document.querySelector('.graph');
    const bb = c._cyreg.cy.elements().renderedBoundingBox({});
    return bb.x1 >= -1 && bb.y1 >= -1 && bb.x2 <= c.clientWidth + 1 && bb.y2 <= c.clientHeight + 1;
  });
  assert.ok(fitted, 'Fit all shows the whole tree');
  await page.click('text=Current');
  await page.goto(pathToFileURL(dist).href);
  await page.waitForSelector('canvas');
}

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
