import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  chooseTexts,
  fenceFor,
  isLicenseFile,
  normalizeLicense,
  npmPackageOf,
  renderNotices,
  shippedCrates,
  sourceUrl,
  textKey,
} from './notices-lib.mjs';

const MIT = 'Copyright (c) 2020 A\n\nPermission is hereby granted, free of charge, to any person';
const APACHE =
  'Apache License\nVersion 2.0, January 2004\n\nTERMS AND CONDITIONS FOR USE, REPRODUCTION';

test('license files are recognised by name', () => {
  for (const name of ['LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE', 'license.md', 'COPYING',
    'NOTICE', 'UNLICENSE', 'LICENCE.txt', 'COPYRIGHT']) {
    assert.ok(isLicenseFile(name), name);
  }
  for (const name of ['README.md', 'Cargo.toml', 'license.rs', 'build.rs', 'licenses.json']) {
    assert.ok(!isLicenseFile(name), name);
  }
});

test('only normal dependency edges count, on any target', () => {
  const nodes = [
    { id: 'app', deps: [
      { pkg: 'a', dep_kinds: [{ kind: null, target: null }] },
      { pkg: 'win', dep_kinds: [{ kind: null, target: 'cfg(windows)' }] },
      { pkg: 'build', dep_kinds: [{ kind: 'build', target: null }] },
      { pkg: 'dev', dep_kinds: [{ kind: 'dev', target: null }] },
    ] },
    { id: 'a', deps: [{ pkg: 'b', dep_kinds: [{ kind: 'dev', target: null }, { kind: null, target: null }] }] },
    { id: 'b', deps: [] },
    { id: 'win', deps: [] },
    { id: 'build', deps: [{ pkg: 'c', dep_kinds: [{ kind: null, target: null }] }] },
    { id: 'c', deps: [] },
    { id: 'dev', deps: [] },
  ];
  assert.deepEqual([...shippedCrates(nodes, ['app'])].sort(), ['a', 'app', 'b', 'win']);
});

test('bundled modules map to their innermost package', () => {
  assert.deepEqual(npmPackageOf('/r/node_modules/svelte/src/index.js'), {
    name: 'svelte',
    dir: '/r/node_modules/svelte',
  });
  assert.deepEqual(npmPackageOf('C:\\r\\node_modules\\@tauri-apps\\api\\core.js?v=1'), {
    name: '@tauri-apps/api',
    dir: 'C:/r/node_modules/@tauri-apps/api',
  });
  assert.equal(npmPackageOf('/r/node_modules/a/node_modules/b/x.js').name, 'b');
  assert.equal(npmPackageOf('/r/src/main.ts'), null);
  assert.equal(npmPackageOf('\0vite/modulepreload-polyfill.js'), null);
});

test('license expressions and source links are normalised', () => {
  assert.equal(normalizeLicense('MIT/Apache-2.0'), 'MIT OR Apache-2.0');
  assert.equal(normalizeLicense('Apache-2.0 / MIT'), 'Apache-2.0 OR MIT');
  assert.equal(normalizeLicense(null), 'unknown');
  assert.equal(sourceUrl({ repository: 'git+https://github.com/x/y.git' }), 'https://github.com/x/y');
  assert.equal(sourceUrl({ repository: { url: 'github:x/y' } }), 'https://github.com/x/y');
  assert.equal(sourceUrl({ repository: 'ssh://git@example.com/y' }), '');
});

test('dual-licensed packages keep MIT and drop the Apache text', () => {
  const texts = [
    { file: 'LICENSE-APACHE', text: APACHE },
    { file: 'LICENSE-MIT', text: MIT },
    { file: 'NOTICE', text: 'Notice text' },
  ];
  assert.deepEqual(chooseTexts('MIT OR Apache-2.0', texts).map((t) => t.file), ['LICENSE-MIT', 'NOTICE']);
  // No MIT copy shipped, an AND, or no MIT choice: everything stays.
  assert.equal(chooseTexts('MIT OR Apache-2.0', [texts[0]]).length, 1);
  assert.equal(chooseTexts('(MIT OR Apache-2.0) AND Unicode-3.0', texts).length, 3);
  assert.equal(chooseTexts('BSD-3-Clause OR Apache-2.0', texts).length, 3);
  assert.equal(chooseTexts('Apache-2.0', texts).length, 3);
});

test('identical texts are printed once, and the output does not depend on input order', () => {
  const pkg = (name, text) => ({ name, version: '1.0.0', license: 'MIT', url: '', texts: [{ file: 'LICENSE', text }] });
  const rust = [pkg('b', MIT), pkg('a', `${MIT.replaceAll('\n', '\r\n')}  \n`), pkg('c', 'Other')];
  const npm = [{ name: 'n', version: '2.0.0', license: 'MIT', url: 'https://x.test', texts: [] }];
  const out = renderNotices({ rust, npm });
  assert.equal(out, renderNotices({ rust: [...rust].reverse(), npm }));
  assert.equal(out.match(/^### Notice/gm).length, 2);
  assert.match(out, /Used by: a 1\.0\.0, b 1\.0\.0\./);
  assert.match(out, /- n 2\.0\.0: MIT \(<https:\/\/x\.test>\)/);
  assert.ok(!out.includes('\r'));
});

test('text keys ignore layout but not names; fences outgrow backticks', () => {
  assert.equal(textKey('A  B\n\n- c'), textKey('a b c'));
  assert.notEqual(textKey('Copyright 2020 A'), textKey('Copyright 2021 A'));
  assert.equal(fenceFor('plain'), '```');
  assert.equal(fenceFor('has ```` four'), '`````');
});
