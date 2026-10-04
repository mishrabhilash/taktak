import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import {
  configProblems,
  DENIED_CRATES,
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

test('Cargo.lock packages are parsed, and the allow-list only covers lock entries', () => {
  const lock = '[[package]]\nname = "reqwest"\nversion = "1"\n\n[[package]]\nname = "ureq"\nversion = "2"\n';
  assert.deepEqual(lockPackages(lock), ['reqwest', 'ureq']);
  assert.deepEqual(deniedIn(lockPackages(lock), DENIED_CRATES, LOCK_ALLOWED), ['ureq']);
  // The tree check uses no allow-list: reqwest in a desktop build fails.
  assert.deepEqual(deniedIn(['reqwest', 'serde'], DENIED_CRATES), ['reqwest']);
  for (const name of Object.keys(LOCK_ALLOWED)) assert.ok(DENIED_CRATES.includes(name), name);
});

test("the repository's own Cargo.lock passes", () => {
  const names = lockPackages(readFileSync(path.join(root, 'Cargo.lock'), 'utf8'));
  assert.ok(names.includes('tauri'));
  assert.deepEqual(deniedIn(names, DENIED_CRATES, LOCK_ALLOWED), []);
});

test('cargo tree output is reduced to crate names', () => {
  const tree = 'taktak v0.1.0 (/x/src-tauri)\nserde v1.0.229\nhyper v1.0.0 (*)\n\n';
  assert.deepEqual(treeCrates(tree), ['hyper', 'serde', 'taktak']);
});

test('network API patterns catch calls, not prose', () => {
  assert.ok(DENIED_WEB_APIS.test('const r = await fetch(url);'));
  assert.ok(DENIED_WEB_APIS.test('new WebSocket("wss://x")'));
  assert.ok(DENIED_WEB_APIS.test('navigator.sendBeacon(u, d)'));
  assert.ok(!DENIED_WEB_APIS.test('// fetch the state from the backend'));
  assert.ok(DENIED_RUST_APIS.test('use std::net::TcpStream;'));
  assert.ok(!DENIED_RUST_APIS.test('use std::os::unix::net::UnixListener;'));
});

test('bundle URLs: namespaces and docs links pass, anything else fails', () => {
  const js = 'a="http://www.w3.org/2000/svg";b=`https://svelte.dev/e/effect_orphan`;c="https://evil.example/t?x=1";d="https://taktak.tech/docs".';
  const urls = urlsIn(js);
  assert.deepEqual(urls, [
    'http://www.w3.org/2000/svg',
    'https://svelte.dev/e/effect_orphan',
    'https://evil.example/t?x=1',
    'https://taktak.tech/docs',
  ]);
  assert.deepEqual(disallowedUrls(urls), ['https://evil.example/t?x=1']);
  assert.deepEqual(disallowedUrls(['https://taktak.tech.evil.example/']), ['https://taktak.tech.evil.example/']);
});

test('tauri.conf.json: connect-src is IPC only', () => {
  const config = JSON.parse(readFileSync(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));
  assert.deepEqual(configProblems(config), []);
  const open = structuredClone(config);
  open.app.security.csp['connect-src'] += ' https://api.example';
  assert.deepEqual(configProblems(open), ['connect-src allows https://api.example']);
  assert.deepEqual(configProblems({ app: { security: { csp: "default-src 'self'" } }, plugins: { updater: {} } }), [
    'app.security.csp must be an object with an explicit connect-src',
    'plugins.updater is configured',
  ]);
});
