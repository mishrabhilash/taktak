// TakTak website. Plain JavaScript, no dependencies, no network requests except this
// site's own files (the CSP enforces it). Nothing typed on the page is stored or sent.
'use strict';

// ---------------------------------------------------------------------------------------
// CONFIG: the one place to change the repository and the release version.
// Every GitHub link on the page (marked data-repo="<path>") and every download link is
// built from this.
// ---------------------------------------------------------------------------------------
const CONFIG = {
  repo: 'https://github.com/mishrabhilash/taktak', // TODO: the real GitHub repository
  version: '0.2.0', // the release the download buttons point at (tag v<version>)
};

const v = CONFIG.version;
const DOWNLOADS = {
  mac: {
    name: 'macOS',
    files: [{ file: `TakTak_${v}_universal.dmg`, label: '.dmg', note: 'macOS 11 or newer · Apple silicon and Intel' }],
  },
  windows: {
    name: 'Windows',
    files: [
      { file: `TakTak_${v}_x64_en-US.msi`, label: '.msi', note: 'Windows 10/11 · x64' },
      { file: `TakTak_${v}_x64-setup.exe`, label: '.exe', note: 'Windows 10/11 · x64' },
    ],
  },
  linux: {
    name: 'Linux',
    files: [
      { file: `TakTak_${v}_amd64.AppImage`, label: '.AppImage', note: 'x64 · X11 or Wayland' },
      { file: `TakTak_${v}_amd64.deb`, label: '.deb', note: 'Debian, Ubuntu · x64' },
    ],
  },
};

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));

function releaseUrl(file) {
  return `${CONFIG.repo}/releases/download/v${CONFIG.version}/${file}`;
}

// ---------------------------------------------------------------------------------------
// Links and downloads
// ---------------------------------------------------------------------------------------
function applyLinks() {
  for (const a of $$('a[data-repo]')) a.href = CONFIG.repo + a.dataset.repo;
  for (const a of $$('a[data-dl]')) {
    const [os, i] = a.dataset.dl.split(':');
    const f = DOWNLOADS[os] && DOWNLOADS[os].files[Number(i)];
    if (f) a.href = releaseUrl(f.file);
  }
  for (const el of $$('[data-dl-name]')) {
    const [os, i] = el.dataset.dlName.split(':');
    const f = DOWNLOADS[os] && DOWNLOADS[os].files[Number(i)];
    if (f) el.textContent = f.file;
  }
  const ver = $('#dl-version');
  if (ver) ver.textContent = `v${CONFIG.version}`;
}

function detectOS() {
  const nav = navigator;
  const platform = ((nav.userAgentData && nav.userAgentData.platform) || nav.platform || '').toLowerCase();
  const ua = (nav.userAgent || '').toLowerCase();
  if (/android|iphone|ipad|ipod/.test(ua) || (nav.userAgentData && nav.userAgentData.mobile)) return 'mobile';
  if (platform.includes('mac') || ua.includes('mac os')) {
    // iPadOS reports itself as a Mac; a Mac has no touch screen.
    return nav.maxTouchPoints > 1 ? 'mobile' : 'mac';
  }
  if (platform.includes('win') || ua.includes('windows')) return 'windows';
  if (platform.includes('linux') || platform.includes('x11') || ua.includes('linux') || ua.includes('cros')) return 'linux';
  return null;
}

function setupDownloadCta() {
  const os = detectOS();
  const cta = $('#cta-primary');
  const label = $('#cta-label');
  const meta = $('#cta-meta');
  const others = $('#cta-others');
  if (!cta) return;

  if (!os || os === 'mobile') {
    cta.href = '#download';
    label.textContent = 'Download for desktop';
    meta.textContent = os === 'mobile'
      ? 'TakTak is a desktop app for macOS, Windows and Linux. Try the sounds right here, though.'
      : 'Free · MIT licensed · macOS, Windows and Linux';
    others.hidden = true;
    return;
  }

  const primary = DOWNLOADS[os].files[0];
  cta.href = releaseUrl(primary.file);
  label.textContent = `Download for ${DOWNLOADS[os].name}`;
  meta.textContent = `v${CONFIG.version} · ${primary.label} · ${primary.note} · Free and open source`;

  others.textContent = '';
  others.append('Other platforms: ');
  const keys = Object.keys(DOWNLOADS).filter((k) => k !== os);
  keys.forEach((k) => {
    const a = document.createElement('a');
    a.href = releaseUrl(DOWNLOADS[k].files[0].file);
    a.textContent = `${DOWNLOADS[k].name} (${DOWNLOADS[k].files.map((f) => f.label).join(', ')})`;
    others.append(a, ' · ');
  });
  const all = document.createElement('a');
  all.href = '#download';
  all.textContent = 'all downloads';
  others.append(all);

  const card = $(`.platform[data-os="${os}"]`);
  if (card) {
    card.classList.add('detected');
    const h = $('h3', card);
    if (h && !$('.detected-tag', h)) {
      const tag = document.createElement('span');
      tag.className = 'visually-hidden detected-tag';
      tag.textContent = ' (your system)';
      h.append(tag);
    }
  }
}

// ---------------------------------------------------------------------------------------
// Audio: Web Audio, files from this site only, loaded on first use.
// ---------------------------------------------------------------------------------------
const HUMANIZE = 0.25; // the app's default share of a pack's pitch/volume variation
let ctx = null;
let master = null;
let volume = 0.8;

function audio() {
  if (!ctx) {
    const AC = window.AudioContext || window.webkitAudioContext;
    if (!AC) return null;
    try {
      ctx = new AC({ latencyHint: 'interactive' });
    } catch (e) {
      ctx = new AC();
    }
    master = ctx.createGain();
    master.gain.value = volume;
    master.connect(ctx.destination);
  }
  if (ctx.state === 'suspended') ctx.resume().catch(() => {});
  return ctx;
}

function decode(buf) {
  return new Promise((resolve, reject) => {
    const p = ctx.decodeAudioData(buf, resolve, reject);
    if (p && typeof p.then === 'function') p.then(resolve, reject);
  });
}

const buffers = new Map(); // url -> Promise<AudioBuffer>
function loadBuffer(url) {
  if (!buffers.has(url)) {
    const p = fetch(url, { credentials: 'omit' })
      .then((r) => {
        if (!r.ok) throw new Error(`${url}: ${r.status}`);
        return r.arrayBuffer();
      })
      .then(decode);
    p.catch(() => buffers.delete(url));
    buffers.set(url, p);
  }
  return buffers.get(url);
}

function packInfo(id) {
  const el = $(`.pack[data-pack="${CSS.escape(id)}"]`);
  const num = (x, d) => (Number.isFinite(Number(x)) ? Number(x) : d);
  return {
    id,
    el,
    name: el ? $('.pack-name', el).textContent : id,
    volume: num(el && el.dataset.volume, 1),
    pitch: num(el && el.dataset.pitch, 0.03),
    varVolume: num(el && el.dataset.varVolume, 0.1),
  };
}

// --- "Click to hear" previews ---------------------------------------------------------
let preview = null; // { id, src, button }

function setPlayButton(button, state) {
  const use = $('use', button);
  const text = $('.btn-text', button);
  button.setAttribute('aria-pressed', state === 'playing' ? 'true' : 'false');
  if (state === 'loading') button.setAttribute('aria-busy', 'true');
  else button.removeAttribute('aria-busy');
  if (use) use.setAttribute('href', state === 'playing' ? '#i-stop' : '#i-play');
  if (text) text.textContent = state === 'playing' ? 'Stop' : state === 'loading' ? 'Loading…' : 'Play';
  const card = button.closest('.pack');
  if (card) card.classList.toggle('playing', state === 'playing');
}

function stopPreview() {
  if (!preview) return;
  const { src, button } = preview;
  preview = null;
  try { if (src) src.stop(); } catch (e) { /* already stopped */ }
  setPlayButton(button, 'idle');
}

function togglePreview(button) {
  const id = button.dataset.preview;
  const same = preview && preview.id === id;
  stopPreview();
  if (same) return;
  if (!audio()) return showNoAudio();
  const info = packInfo(id);
  const token = { id, src: null, button };
  preview = token;
  setPlayButton(button, 'loading');
  loadBuffer(`assets/sounds/${id}/preview.mp3`).then((buf) => {
    if (preview !== token) return; // stopped or replaced while loading
    const src = ctx.createBufferSource();
    const gain = ctx.createGain();
    src.buffer = buf;
    gain.gain.value = info.volume;
    src.connect(gain).connect(master);
    src.onended = () => {
      if (preview === token) { preview = null; setPlayButton(button, 'idle'); }
    };
    token.src = src;
    src.start();
    setPlayButton(button, 'playing');
  }).catch(() => {
    if (preview === token) { preview = null; setPlayButton(button, 'idle'); }
    setStatus('Could not load that preview. Try reloading the page.');
  });
}

// --- Keystrokes ---------------------------------------------------------------------
// Each pack's sounds are in one MP3 (an audio sprite) plus keys.json: the start and length
// of every sound, and for every key the sound it plays on press and on release, resolved
// exactly as the app does (same key, same sound). See tools/build_sounds.py.
const packs = new Map(); // id -> Promise<{ buf, meta, offset, info }>

function loadPack(id) {
  if (!packs.has(id)) {
    if (!audio()) return Promise.reject(new Error('no Web Audio'));
    const p = Promise.all([
      fetch(`assets/sounds/${id}/keys.json`, { credentials: 'omit' }).then((r) => {
        if (!r.ok) throw new Error(r.status);
        return r.json();
      }),
      loadBuffer(`assets/sounds/${id}/keys.mp3`),
    ]).then(([meta, buf]) => ({ buf, meta, offset: alignment(buf, meta), info: packInfo(id) }));
    p.catch(() => packs.delete(id));
    packs.set(id, p);
  }
  return packs.get(id);
}

// MP3 decoders may or may not remove the encoder's leading padding. Find the first sound's
// attack in the decoded audio and shift every start by the difference.
function alignment(buf, meta) {
  const data = buf.getChannelData(0);
  const rate = buf.sampleRate;
  const end = Math.min(data.length, Math.round((meta.align + 0.25) * rate));
  const level = meta.alignLevel * 0.9;
  for (let i = 0; i < end; i++) {
    if (Math.abs(data[i]) >= level) {
      const offset = i / rate - meta.align;
      return Math.max(-0.05, Math.min(0.15, offset));
    }
  }
  return 0;
}

const loaded = new Map(); // id -> resolved pack, for synchronous playback on keydown

function playKey(pack, code, action) {
  const pair = pack.meta.keys[code];
  if (!pair) return;
  const index = pair[action === 'press' ? 0 : 1];
  if (index < 0) return;
  const [start, dur] = pack.meta.slices[index];
  const { info } = pack;
  const rand = () => Math.random() * 2 - 1;
  const src = ctx.createBufferSource();
  const gain = ctx.createGain();
  src.buffer = pack.buf;
  src.playbackRate.value = 1 + rand() * info.pitch * HUMANIZE;
  gain.gain.value = info.volume * (1 + rand() * info.varVolume * HUMANIZE);
  src.connect(gain).connect(master);
  src.start(0, Math.max(0, start + pack.offset), dur + 0.005);
}

// --- The "type here" box --------------------------------------------------------------
let selectedPack = 'tactile';
const held = new Set();
let lastKeydown = 0;

function setStatus(text) {
  const s = $('#try-status');
  if (s) s.textContent = text;
}

function showNoAudio() {
  setStatus('Your browser has no Web Audio support, so the sounds cannot play here.');
}

function selectPack(id, { announce = true } = {}) {
  selectedPack = id;
  for (const el of $$('.pack')) el.classList.toggle('selected', el.dataset.pack === id);
  const sel = $('#try-pack');
  if (sel && sel.value !== id) sel.value = id;
  return preparePack(id, announce);
}

function preparePack(id, announce = true) {
  if (loaded.has(id)) {
    if (announce) setStatus(`${loaded.get(id).info.name} is ready. Type away.`);
    return Promise.resolve(loaded.get(id));
  }
  if (!audio()) { showNoAudio(); return Promise.resolve(null); }
  const name = packInfo(id).name;
  if (announce) setStatus(`Loading ${name}…`);
  return loadPack(id).then((pack) => {
    loaded.set(id, pack);
    if (announce && selectedPack === id) setStatus(`${name} is ready. Type away.`);
    return pack;
  }).catch(() => {
    if (selectedPack === id) setStatus(`Could not load ${name}. Try reloading the page.`);
    return null;
  });
}

function hit(code, action) {
  const pack = loaded.get(selectedPack);
  const key = $(`.k[data-code="${code}"]`);
  if (key) key.classList.toggle('down', action === 'press');
  if (!pack) { preparePack(selectedPack); return; }
  audio();
  playKey(pack, code, action);
}

// Characters to key codes, for on-screen keyboards that send no key codes (most phones).
const CHAR_CODES = {
  ' ': 'Space', '`': 'Backquote', '-': 'Minus', '=': 'Equal', '[': 'BracketLeft', ']': 'BracketRight',
  '\\': 'Backslash', ';': 'Semicolon', "'": 'Quote', ',': 'Comma', '.': 'Period', '/': 'Slash',
  '~': 'Backquote', '_': 'Minus', '+': 'Equal', '{': 'BracketLeft', '}': 'BracketRight', '|': 'Backslash',
  ':': 'Semicolon', '"': 'Quote', '<': 'Comma', '>': 'Period', '?': 'Slash', '!': 'Digit1', '@': 'Digit2',
  '#': 'Digit3', '$': 'Digit4', '%': 'Digit5', '^': 'Digit6', '&': 'Digit7', '*': 'Digit8', '(': 'Digit9', ')': 'Digit0',
};
function codeForChar(ch) {
  if (/^[a-z]$/i.test(ch)) return `Key${ch.toUpperCase()}`;
  if (/^[0-9]$/.test(ch)) return `Digit${ch}`;
  return CHAR_CODES[ch] || 'KeyA';
}

function setupTryBox() {
  const input = $('#try-input');
  const sel = $('#try-pack');
  const vol = $('#try-volume');
  if (!input || !sel) return;
  selectedPack = sel.value || selectedPack;

  input.addEventListener('focus', () => preparePack(selectedPack));
  input.addEventListener('keydown', (e) => {
    if (e.repeat || !e.code || e.code === 'Unidentified') return; // auto-repeat plays once, like the app
    lastKeydown = performance.now();
    if (held.has(e.code)) return;
    held.add(e.code);
    hit(e.code, 'press');
  });
  input.addEventListener('keyup', (e) => {
    if (!held.delete(e.code)) return;
    hit(e.code, 'release');
  });
  input.addEventListener('blur', () => {
    for (const code of held) {
      const key = $(`.k[data-code="${code}"]`);
      if (key) key.classList.remove('down');
    }
    held.clear();
  });
  // Phones: the on-screen keyboard often sends keydown without a key code. Fall back to
  // the input event and play a press and a release for what was typed.
  input.addEventListener('input', (e) => {
    if (performance.now() - lastKeydown < 150) return;
    if (e.inputType === 'insertFromPaste' || e.inputType === 'insertFromDrop' || e.inputType === 'historyUndo') return;
    let code = 'KeyA';
    if (e.inputType === 'insertLineBreak' || e.inputType === 'insertParagraph') code = 'Enter';
    else if (e.inputType && e.inputType.startsWith('delete')) code = 'Backspace';
    else if (e.data) code = codeForChar(e.data.slice(-1));
    hit(code, 'press');
    setTimeout(() => hit(code, 'release'), 70);
  });

  sel.addEventListener('change', () => selectPack(sel.value));
  if (vol) {
    const apply = () => {
      volume = Number(vol.value) / 100;
      if (master) master.gain.value = volume;
    };
    vol.addEventListener('input', apply);
    apply();
  }

  for (const b of $$('[data-try]')) {
    b.addEventListener('click', () => {
      selectPack(b.dataset.try);
      input.focus({ preventScroll: true });
      const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
      $('#try').scrollIntoView({ behavior: reduce ? 'auto' : 'smooth', block: 'start' });
    });
  }
  for (const b of $$('[data-preview]')) b.addEventListener('click', () => togglePreview(b));
}

// The hero keyboard: press a key with the mouse or a finger to hear it.
function setupHeroKeyboard() {
  const kbd = $('#kbd');
  if (!kbd) return;
  let down = null;
  const release = () => {
    if (!down) return;
    hit(down, 'release');
    down = null;
  };
  kbd.addEventListener('pointerdown', (e) => {
    const key = e.target.closest('.k');
    if (!key) return;
    e.preventDefault();
    release();
    down = key.dataset.code;
    if (!loaded.has(selectedPack)) {
      key.classList.add('down');
      preparePack(selectedPack, false).then((pack) => {
        if (pack && down === key.dataset.code) playKey(pack, down, 'press');
      });
      return;
    }
    hit(down, 'press');
  });
  kbd.addEventListener('pointerup', release);
  kbd.addEventListener('pointerleave', release);
  kbd.addEventListener('pointercancel', release);
}

document.addEventListener('visibilitychange', () => {
  if (document.hidden) stopPreview();
});

applyLinks();
setupDownloadCta();
setupTryBox();
setupHeroKeyboard();
