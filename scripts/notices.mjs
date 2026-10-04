#!/usr/bin/env node
// Writes THIRD_PARTY_NOTICES.md: the third-party code compiled into the TakTak app, with the
// license texts and notices it ships. Works offline.
//
//   npm run notices              regenerate THIRD_PARTY_NOTICES.md
//   npm run notices -- --check   exit 1 if THIRD_PARTY_NOTICES.md is out of date (CI)
//
// Rust: `cargo metadata --locked --offline`, filtered to the desktop targets TakTak is released
// for (DESKTOP_TARGETS; one file covers every OS), walked from the `taktak` app crate along
// normal dependencies; license texts are read from each crate's source folder in the Cargo
// registry. If cargo reports a missing
// crate, run `cargo fetch --locked` once (that is the only step that downloads anything).
//
// npm: a production Vite build in memory (nothing is written to dist/) reports which modules
// end up in the UI bundle; each one's package.json gives the version and license, and its
// folder the license texts.
//
// The output depends only on Cargo.lock, package-lock.json and the sources, never on the
// machine, so CI can check that the committed file is current.

import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import {
  isLicenseFile,
  LICENSE_DIR,
  normalizeLicense,
  npmPackageOf,
  renderNotices,
  shippedCrates,
  sourceUrl,
} from './notices-lib.mjs';

const root = path.resolve(import.meta.dirname, '..');
const output = path.join(root, 'THIRD_PARTY_NOTICES.md');
const check = process.argv.includes('--check');

/** The targets release.yml builds: macOS universal (both), Windows x64, Linux x64. */
export const DESKTOP_TARGETS = [
  'aarch64-apple-darwin',
  'x86_64-apple-darwin',
  'x86_64-pc-windows-msvc',
  'x86_64-unknown-linux-gnu',
];

/** License files at the top of `dir` (and in a LICENSES/ folder), as `{ file, text }`. */
function licenseTexts(dir, extra = []) {
  const found = new Map();
  const add = (rel) => {
    const full = path.join(dir, rel);
    try {
      if (statSync(full).isFile() && !found.has(rel)) found.set(rel, readFileSync(full, 'utf8'));
    } catch {
      // A license_file that does not exist in the published package: nothing to add.
    }
  };
  let entries = [];
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return [];
  }
  for (const entry of entries) {
    if (entry.isFile() && isLicenseFile(entry.name)) add(entry.name);
    if (entry.isDirectory() && LICENSE_DIR.test(entry.name)) {
      for (const inner of readdirSync(path.join(dir, entry.name))) add(`${entry.name}/${inner}`);
    }
  }
  for (const rel of extra) if (rel) add(rel.replaceAll('\\', '/'));
  return [...found]
    .sort(([a], [b]) => a.localeCompare(b, 'en'))
    .map(([file, text]) => ({ file, text }));
}

function rustCrates() {
  const cargo = spawnSync(
    'cargo',
    [
      'metadata',
      '--format-version',
      '1',
      '--locked',
      '--offline',
      ...DESKTOP_TARGETS.flatMap((t) => ['--filter-platform', t]),
    ],
    { cwd: root, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 },
  );
  if (cargo.status !== 0) {
    console.error(cargo.stderr);
    console.error('notices: cargo metadata failed. If a crate is missing, run `cargo fetch --locked` once.');
    process.exit(1);
  }
  const meta = JSON.parse(cargo.stdout);
  const app = meta.packages.find((p) => p.name === 'taktak' && p.source === null);
  if (!app) throw new Error('notices: the taktak app crate is not in the workspace');
  const shipped = shippedCrates(meta.resolve.nodes, [app.id]);
  return meta.packages
    .filter((p) => shipped.has(p.id) && p.source !== null) // our own path crates are MIT, see LICENSE
    .map((p) => {
      const dir = path.dirname(p.manifest_path);
      return {
        name: p.name,
        version: p.version,
        license: normalizeLicense(p.license ?? (p.license_file ? `see ${p.license_file}` : null)),
        url: sourceUrl(p) || `https://crates.io/crates/${p.name}/${p.version}`,
        texts: licenseTexts(dir, [p.license_file]),
      };
    });
}

async function npmPackages() {
  // The shipped UI: what `tauri build` builds (no mock backend, see vite.config.ts).
  process.env.TAURI_ENV_PLATFORM ||= 'darwin';
  const { build } = await import('vite');
  const ids = new Set();
  await build({
    root,
    configFile: path.join(root, 'vite.config.ts'),
    logLevel: 'error',
    mode: 'production',
    build: { write: false, emptyOutDir: false },
    plugins: [
      {
        name: 'taktak-notices',
        generateBundle(_options, bundle) {
          for (const chunk of Object.values(bundle)) {
            if (chunk.type !== 'chunk') continue;
            for (const [id, info] of Object.entries(chunk.modules ?? {})) {
              if (!info || info.renderedLength > 0) ids.add(id);
            }
          }
        },
      },
    ],
  });
  const packages = new Map();
  for (const id of ids) {
    const pkg = npmPackageOf(id);
    if (!pkg || packages.has(pkg.dir)) continue;
    const manifestPath = path.join(pkg.dir, 'package.json');
    if (!existsSync(manifestPath)) continue;
    const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
    packages.set(pkg.dir, {
      name: manifest.name ?? pkg.name,
      version: manifest.version ?? 'unknown',
      license: normalizeLicense(
        typeof manifest.license === 'object' ? manifest.license?.type : manifest.license,
      ),
      url: sourceUrl(manifest) || `https://www.npmjs.com/package/${manifest.name ?? pkg.name}`,
      texts: licenseTexts(pkg.dir),
    });
  }
  return [...packages.values()];
}

const notices = renderNotices({ rust: rustCrates(), npm: await npmPackages() });
const current = existsSync(output) ? readFileSync(output, 'utf8') : '';
if (check) {
  if (current === notices) {
    console.log('notices: THIRD_PARTY_NOTICES.md is up to date');
  } else {
    console.error('notices: THIRD_PARTY_NOTICES.md is out of date: run `npm run notices` and commit it');
    process.exit(1);
  }
} else {
  writeFileSync(output, notices);
  console.log(`notices: wrote THIRD_PARTY_NOTICES.md (${Math.round(notices.length / 1024)} KB)`);
}
