#!/usr/bin/env node
// The "no network" guard: TakTak never uses the internet, and CI keeps it that way.
//
//   npm run no-network                    every check (needs `cargo fetch --locked` once, and a
//                                         shipped UI build in dist/, see below)
//   npm run no-network -- --skip-dist     without the UI bundle check
//   npm run no-network -- --skip-tree     without the cargo tree check (no cargo needed)
//
// Checks, all offline:
//   1. Cargo.lock holds no networking crate (DENIED_CRATES) except the allow-listed ones that
//      only mobile targets use (LOCK_ALLOWED, each with its reason).
//   2. `cargo tree` for every desktop target, all workspace crates, normal, build and dev
//      dependencies: no networking crate at all, no allow-list.
//   3. TakTak's Rust sources use no std::net; the UI sources call no fetch/XHR/WebSocket/…;
//      package.json depends on no networking npm package.
//   4. tauri.conf.json: connect-src reaches nothing but Tauri's IPC; no updater or HTTP plugin.
//   5. The shipped UI bundle (dist/) contains no http(s) URL except the allowed identifiers and
//      documentation/license links (ALLOWED_URLS). Build it as `tauri build` does, without the
//      browser mock: TAURI_ENV_PLATFORM=linux npm run build (any platform name works).

import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import {
  configProblems,
  DENIED_CRATES,
  DENIED_NPM,
  DENIED_RUST_APIS,
  DENIED_WEB_APIS,
  deniedIn,
  disallowedUrls,
  LOCK_ALLOWED,
  lockPackages,
  treeCrates,
  urlsIn,
} from './no-network-lib.mjs';

const root = path.resolve(import.meta.dirname, '..');
const args = process.argv.slice(2);
const DESKTOP_TARGETS = [
  'aarch64-apple-darwin',
  'x86_64-apple-darwin',
  'x86_64-pc-windows-msvc',
  'aarch64-pc-windows-msvc',
  'x86_64-unknown-linux-gnu',
  'aarch64-unknown-linux-gnu',
];

const failures = [];
const fail = (message) => failures.push(message);
const ok = (message) => console.log(`no-network: ok   ${message}`);

/** Files under `dir` (relative to root) whose names match `pattern`, skipping build output. */
function files(dir, pattern) {
  const out = [];
  const walk = (abs) => {
    for (const entry of readdirSync(abs, { withFileTypes: true })) {
      if (['node_modules', 'target', 'dist', 'gen', '.git'].includes(entry.name)) continue;
      const full = path.join(abs, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (pattern.test(entry.name)) out.push(full);
    }
  };
  if (existsSync(path.join(root, dir))) walk(path.join(root, dir));
  return out.sort();
}

const rel = (file) => path.relative(root, file).replaceAll('\\', '/');

// 1. Cargo.lock
{
  const names = lockPackages(readFileSync(path.join(root, 'Cargo.lock'), 'utf8'));
  const denied = deniedIn(names, DENIED_CRATES, LOCK_ALLOWED);
  if (denied.length > 0) {
    fail(`Cargo.lock contains networking crates: ${denied.join(', ')}. TakTak must not use the network.`);
  } else {
    const allowed = names.filter((n) => Object.hasOwn(LOCK_ALLOWED, n));
    ok(`Cargo.lock: no networking crates${allowed.length ? ` (mobile-only, allowed: ${allowed.join(', ')})` : ''}`);
  }
}

// 2. Desktop dependency trees
if (!args.includes('--skip-tree')) {
  for (const target of DESKTOP_TARGETS) {
    const tree = spawnSync(
      'cargo',
      ['tree', '--workspace', '--locked', '--offline', '-e', 'normal,build,dev', '--target', target, '--prefix', 'none'],
      { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 },
    );
    if (tree.status !== 0) {
      fail(`cargo tree --target ${target} failed (run \`cargo fetch --locked\` once):\n${tree.stderr}`);
      continue;
    }
    const denied = deniedIn(treeCrates(tree.stdout), DENIED_CRATES);
    if (denied.length > 0) fail(`${target}: the build depends on networking crates: ${denied.join(', ')}`);
    else ok(`${target}: no networking crates in the dependency tree`);
  }
}

// 3. Sources and package.json
{
  const hits = [];
  for (const file of [...files('src-tauri', /\.rs$/), ...files('tools', /\.rs$/)]) {
    readFileSync(file, 'utf8')
      .split('\n')
      .forEach((line, i) => {
        if (DENIED_RUST_APIS.test(line)) hits.push(`${rel(file)}:${i + 1}: ${line.trim()}`);
      });
  }
  for (const file of files('src', /\.(ts|js|svelte|mjs)$/)) {
    readFileSync(file, 'utf8')
      .split('\n')
      .forEach((line, i) => {
        if (DENIED_WEB_APIS.test(line)) hits.push(`${rel(file)}:${i + 1}: ${line.trim()}`);
      });
  }
  if (hits.length > 0) fail(`network APIs in TakTak's sources:\n  ${hits.join('\n  ')}`);
  else ok('sources: no std::net, fetch, XMLHttpRequest, WebSocket, EventSource or sendBeacon');

  const pkg = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8'));
  const deps = Object.keys({ ...pkg.dependencies, ...pkg.devDependencies });
  const denied = deniedIn(deps, DENIED_NPM);
  if (denied.length > 0) fail(`package.json depends on networking packages: ${denied.join(', ')}`);
  else ok('package.json: no networking packages');
}

// 4. Tauri config
{
  const config = JSON.parse(readFileSync(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));
  const problems = configProblems(config);
  if (problems.length > 0) fail(`src-tauri/tauri.conf.json: ${problems.join('; ')}`);
  else ok("tauri.conf.json: connect-src is Tauri's IPC only; no updater or HTTP plugins");
}

// 5. The shipped UI bundle
if (!args.includes('--skip-dist')) {
  const dist = path.join(root, 'dist');
  const bundle = files('dist', /\.(js|css|html|json|svg)$/);
  if (!existsSync(dist) || bundle.length === 0) {
    fail('dist/ is empty: build the shipped UI first (TAURI_ENV_PLATFORM=linux npm run build)');
  } else if (bundle.some((f) => /\/mock-[^/]*\.js$/.test(rel(f)))) {
    fail('dist/ holds the browser mock (a dev build): rebuild with TAURI_ENV_PLATFORM=linux npm run build');
  } else {
    const bad = [];
    for (const file of bundle) {
      for (const url of disallowedUrls(urlsIn(readFileSync(file, 'utf8')))) bad.push(`${rel(file)}: ${url}`);
    }
    if (bad.length > 0) {
      fail(`URLs in the UI bundle that are not on the allow-list (scripts/no-network-lib.mjs):\n  ${bad.join('\n  ')}`);
    } else {
      ok(`dist/: ${bundle.length} files, no URLs beyond namespaces and docs/license links`);
    }
  }
}

if (failures.length > 0) {
  for (const message of failures) console.error(`no-network: FAIL ${message}`);
  console.error('no-network: TakTak is an offline app. See CONTRIBUTING.md § Privacy rules.');
  process.exit(1);
}
console.log('no-network: TakTak still never touches the network.');
