// Turn an SVG picture into a PNG with an installed Chrome (nothing is downloaded).
//   node scripts/svg-to-png.mjs ../docs/screenshots/watch-demo.svg ../docs/screenshots/watch-demo.png
import { chromium } from 'playwright-core';
import { existsSync, readFileSync } from 'node:fs';

const [input, output] = process.argv.slice(2);
if (!input || !output) {
  console.error('usage: node scripts/svg-to-png.mjs <input.svg> <output.png>');
  process.exit(1);
}
const executablePath = [process.env.CHROME_PATH, '/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser']
  .filter(Boolean)
  .find((p) => existsSync(p));
if (!executablePath) {
  console.error('No Chrome or Chromium found. Set CHROME_PATH.');
  process.exit(1);
}
const svg = readFileSync(input, 'utf8');
const m = /width="(\d+)" height="(\d+)"/.exec(svg);
const [w, h] = m ? [Number(m[1]), Number(m[2])] : [1200, 700];
const browser = await chromium.launch({ executablePath, args: ['--no-sandbox'] });
const page = await browser.newPage({ viewport: { width: w, height: h }, deviceScaleFactor: 2 });
await page.setContent(`<!doctype html><body style="margin:0;background:#1e1e2e">${svg}</body>`);
await page.screenshot({ path: output, clip: { x: 0, y: 0, width: w, height: h } });
await browser.close();
console.log('wrote', output);
