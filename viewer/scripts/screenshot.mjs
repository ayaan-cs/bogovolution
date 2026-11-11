// Take screenshots of the built viewer with the bundled demo loaded.
//
//   npm run build && npm run screenshots
//
// Uses playwright-core with a Chrome or Chromium that is already installed
// (nothing is downloaded). Set CHROME_PATH if it is not at a common location.
// Output goes to ../docs/screenshots/.
import { chromium } from 'playwright-core';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const dist = join(root, 'dist', 'index.html');
const outDir = resolve(root, '..', 'docs', 'screenshots');

const candidates = [
  process.env.CHROME_PATH,
  '/usr/bin/google-chrome',
  '/usr/bin/google-chrome-stable',
  '/usr/bin/chromium',
  '/usr/bin/chromium-browser',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
].filter(Boolean);
const executablePath = candidates.find((p) => existsSync(p));
if (!executablePath) {
  console.error('No Chrome or Chromium found. Set CHROME_PATH.');
  process.exit(1);
}
if (!existsSync(dist)) {
  console.error('dist/index.html is missing. Run npm run build first.');
  process.exit(1);
}
mkdirSync(outDir, { recursive: true });

const tree = JSON.parse(readFileSync(join(root, 'src', 'demo', 'tree.json'), 'utf8'));
const rejected = tree.nodes.find((n) => n.status === 'rejected' && /skipping/i.test(n.hypothesis ?? ''))
  ?? tree.nodes.find((n) => n.status === 'rejected');

const browser = await chromium.launch({ executablePath, args: ['--no-sandbox'] });
const problems = [];

async function shot(name, { scheme, hash, fullPage }) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: scheme, deviceScaleFactor: 1 });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => problems.push(`${name}: ${e.message}`));
  page.on('console', (m) => m.type() === 'error' && problems.push(`${name}: ${m.text()}`));
  page.on('request', (r) => {
    if (!r.url().startsWith('file:') && !r.url().startsWith('data:')) problems.push(`${name}: network request ${r.url()}`);
  });
  await page.goto(pathToFileURL(dist).href + (hash ? `#v=${hash}` : ''));
  await page.waitForSelector('canvas');
  await page.waitForSelector('.d2h-wrapper, .details');
  await page.waitForTimeout(900);
  const file = join(outDir, name);
  await page.screenshot({ path: file, fullPage });
  console.log('wrote', file);
  await ctx.close();
}

await shot('viewer-light.png', { scheme: 'light', fullPage: false });
await shot('viewer-dark-rejected.png', { scheme: 'dark', hash: rejected?.hash.slice(0, 12), fullPage: true });
await browser.close();

if (problems.length > 0) {
  console.error('Problems while loading the page:\n' + problems.join('\n'));
  process.exit(1);
}
