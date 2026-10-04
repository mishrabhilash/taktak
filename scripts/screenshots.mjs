// Retakes the UI screenshots used by README.md and the website, from the browser mock.
// Development tooling only, never shipped: it drives a local headless Chrome through its
// DevTools port on 127.0.0.1 and loads nothing but the local dev server.
//
//   npm run dev            # in another terminal: the mock UI at http://localhost:1420
//   node scripts/screenshots.mjs [--chrome /path/to/chrome]
//
// Writes docs/assets/screenshots/<view>-<light|dark>.png (2x, framed like a macOS window on a
// transparent background) and, when `cwebp` is on PATH, smaller WebP copies for the website in
// website/assets/screenshots/. It also re-renders the social images from their SVG sources:
// website/assets/og.png (1200×630) and docs/assets/social-preview.png (1280×640).

import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const arg = (name) => {
  const i = process.argv.indexOf(name);
  return i > 0 ? process.argv[i + 1] : undefined;
};
const CHROME = arg('--chrome') ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const DEV = 'http://localhost:1420/';
const PORT = 9333;

// Each view: URL suffix on the dev server, window size (as in src-tauri/src/windows.rs), the
// title bar text, the height to keep (crops empty space) and whether the window is fixed-size.
const VIEWS = [
  { name: 'tray', url: '?window=tray', w: 340, h: 460 },
  { name: 'sounds', url: '#sounds', w: 820, h: 600, title: 'TakTak Settings' },
  { name: 'apps', url: '?scenario=rules#apps', w: 820, h: 600, title: 'TakTak Settings' },
  { name: 'volume', url: '#volume', w: 820, h: 600, title: 'TakTak Settings', keep: 480 },
  { name: 'welcome', url: '?window=onboarding', w: 480, h: 440, title: 'Welcome to TakTak', fixed: true },
];
// Website copies: pixel width of the WebP (the PNGs are 2x).
const WEB_WIDTH = { tray: 560, sounds: 1000, apps: 1000, volume: 1000, welcome: 720 };

async function chrome() {
  const profile = mkdtempSync(path.join(tmpdir(), 'taktak-shots-'));
  const proc = spawn(CHROME, ['--headless=new', `--remote-debugging-port=${PORT}`, `--user-data-dir=${profile}`,
    '--no-first-run', '--no-default-browser-check', '--hide-scrollbars', '--disable-extensions',
    '--allow-file-access-from-files', 'about:blank'], { stdio: 'ignore' });
  let page;
  for (let i = 0; i < 50 && !page; i++) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json();
      page = list.find((t) => t.type === 'page');
    } catch {
      await new Promise((r) => setTimeout(r, 200));
    }
  }
  if (!page) throw new Error(`Chrome did not start (${CHROME}); pass --chrome <path>`);
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener('open', r, { once: true }));
  let id = 0;
  const pending = new Map();
  const waiters = [];
  ws.addEventListener('message', (ev) => {
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      const { res, rej } = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) rej(new Error(JSON.stringify(msg.error)));
      else res(msg.result);
    } else {
      for (const w of [...waiters]) if (w.method === msg.method) { waiters.splice(waiters.indexOf(w), 1); w.res(); }
    }
  });
  const send = (method, params = {}) =>
    new Promise((res, rej) => { pending.set(++id, { res, rej }); ws.send(JSON.stringify({ id, method, params })); });
  await send('Page.enable');
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  return {
    async goto(url, settle = 300) {
      const loaded = new Promise((res) => waiters.push({ method: 'Page.loadEventFired', res }));
      await send('Page.navigate', { url });
      await loaded;
      await sleep(settle);
    },
    size: (width, height, deviceScaleFactor) =>
      send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor, mobile: false }),
    scheme: (value) => send('Emulation.setEmulatedMedia', {
      features: [{ name: 'prefers-color-scheme', value }, { name: 'prefers-reduced-motion', value: 'reduce' }],
    }),
    transparent: (on) => send('Emulation.setDefaultBackgroundColorOverride', on ? { color: { r: 0, g: 0, b: 0, a: 0 } } : {}),
    async png() { return Buffer.from((await send('Page.captureScreenshot', { format: 'png' })).data, 'base64'); },
    close() { ws.close(); proc.kill(); setTimeout(() => rmSync(profile, { recursive: true, force: true }), 500); },
  };
}

/** A page that shows a 2x screenshot inside a macOS-like window frame with a soft shadow. */
function framePage(view, theme, png) {
  const dark = theme === 'dark';
  const TB = view.title ? 28 : 0;
  const keep = view.keep ?? view.h;
  const pad = { x: 28, top: 18, bottom: 40 };
  const off = dark ? '#5a5a5e' : '#d0d0d3';
  const lights = view.fixed ? ['#ff5f57', off, off] : ['#ff5f57', '#febc2e', '#28c840'];
  const width = view.w + pad.x * 2;
  const height = keep + TB + pad.top + pad.bottom;
  const bar = view.title
    ? `<div class="tb">${lights.map((c, i) => `<span class="tl" style="left:${12 + i * 20}px;background:${c}"></span>`).join('')}${view.title}</div>`
    : '';
  const html = `<!doctype html><style>
    html,body{margin:0;background:transparent}
    .win{position:absolute;left:${pad.x}px;top:${pad.top}px;width:${view.w}px;height:${keep + TB}px;border-radius:12px;overflow:hidden;
      background:${dark ? '#1e1e20' : '#f5f5f7'};
      box-shadow:0 0 0 ${dark ? '1px rgba(255,255,255,.14)' : '.5px rgba(0,0,0,.22)'},0 18px 36px rgba(0,0,0,${dark ? '.55' : '.2'}),0 3px 8px rgba(0,0,0,${dark ? '.35' : '.1'})}
    .tb{height:${TB}px;position:relative;box-sizing:border-box;text-align:center;
      background:${dark ? '#2c2c2f' : '#e9e9ec'};border-bottom:1px solid ${dark ? 'rgba(0,0,0,.45)' : 'rgba(0,0,0,.1)'};
      font:600 13px/28px -apple-system,BlinkMacSystemFont,sans-serif;color:${dark ? '#c9c9ce' : '#4a4a4f'}}
    .tl{position:absolute;top:8px;width:12px;height:12px;border-radius:50%;box-shadow:inset 0 0 0 .5px rgba(0,0,0,.18)}
    .clip{height:${keep}px;overflow:hidden}
    img{display:block;width:${view.w}px;height:${view.h}px}
  </style><div class="win">${bar}<div class="clip"><img src="data:image/png;base64,${png.toString('base64')}"></div></div>`;
  return { width, height, url: `data:text/html;base64,${Buffer.from(html).toString('base64')}` };
}

const docsDir = path.join(root, 'docs/assets/screenshots');
const webDir = path.join(root, 'website/assets/screenshots');
mkdirSync(docsDir, { recursive: true });
mkdirSync(webDir, { recursive: true });
try {
  await fetch(DEV);
} catch {
  console.error(`No dev server at ${DEV}: run \`npm run dev\` first.`);
  process.exit(1);
}
const hasWebp = spawnSync('cwebp', ['-version']).status === 0;
const b = await chrome();
try {
  for (const view of VIEWS) {
    for (const theme of ['light', 'dark']) {
      await b.transparent(false);
      await b.scheme(theme);
      await b.size(view.w, view.h, 2);
      await b.goto('about:blank', 0);
      await b.goto(DEV + view.url, 1200);
      const raw = await b.png();
      const frame = framePage(view, theme, raw);
      await b.transparent(true);
      await b.size(frame.width, frame.height, 2);
      await b.goto(frame.url, 200);
      const file = path.join(docsDir, `${view.name}-${theme}.png`);
      writeFileSync(file, await b.png());
      if (hasWebp) {
        const out = path.join(webDir, `${view.name}-${theme}.webp`);
        spawnSync('cwebp', ['-quiet', '-q', '80', '-alpha_q', '85', '-m', '6', '-resize', String(WEB_WIDTH[view.name]), '0', file, '-o', out]);
      }
      console.log(`  ${path.relative(root, file)}`);
    }
  }
  if (!hasWebp) console.log('cwebp not found: website/assets/screenshots/*.webp left as they were');

  // Social images from their SVG sources, rendered at their own size.
  await b.transparent(false);
  await b.scheme('light');
  for (const [svg, png, w, h] of [
    ['website/assets/og.svg', 'website/assets/og.png', 1200, 630],
    ['docs/assets/social-preview.svg', 'docs/assets/social-preview.png', 1280, 640],
  ]) {
    await b.size(w, h, 1);
    const html = `<!doctype html><style>html,body{margin:0}img{display:block}</style><img src="file://${path.join(root, svg)}" width="${w}" height="${h}">`;
    const page = path.join(tmpdir(), 'taktak-social.html');
    writeFileSync(page, html);
    await b.goto(`file://${page}`, 300);
    writeFileSync(path.join(root, png), await b.png());
    console.log(`  ${png}`);
  }
} finally {
  b.close();
}
