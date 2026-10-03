// node --test scripts/*.test.mjs
import assert from 'node:assert/strict';
import path from 'node:path';
import { test } from 'node:test';
import { identityHashes, targetDir } from './signing.mjs';

const A = 'ACCB194F0123456789ABCDEF0123456789ABCDEF';
const B = '455A65770123456789ABCDEF0123456789ABCDEF';

test('one identity is found by name', () => {
  const out = `  1) ${A} "TakTak Development"\n     1 valid identities found\n`;
  assert.deepEqual(identityHashes(out, 'TakTak Development'), [A]);
});

test('two certificates with the same name are both reported, in listing order', () => {
  const out =
    `  1) ${A} "TakTak Development"\n` +
    `  2) ${B} "TakTak Development"\n` +
    '     2 valid identities found\n';
  assert.deepEqual(identityHashes(out, 'TakTak Development'), [A, B]);
});

test('the same certificate listed twice counts once', () => {
  const out = `  1) ${A} "TakTak Development"\n  2) ${A} "TakTak Development"\n`;
  assert.deepEqual(identityHashes(out, 'TakTak Development'), [A]);
});

test('other names, prefixes and suffixes do not match', () => {
  const out =
    `  1) ${A} "TakTak Development Old"\n` +
    `  2) ${B} "Apple Development: TakTak Development"\n`;
  assert.deepEqual(identityHashes(out, 'TakTak Development'), []);
  assert.deepEqual(identityHashes('0 valid identities found\n', 'TakTak Development'), []);
});

test('untrusted identities (no -v) are matched despite the status suffix', () => {
  const out = `  1) ${A} "TakTak Development" (CSSMERR_TP_NOT_TRUSTED)\n`;
  assert.deepEqual(identityHashes(out, 'TakTak Development'), [A]);
});

test('an identity given as a hash matches by hash, ignoring case', () => {
  const out = `  1) ${A} "TakTak Development"\n  2) ${B} "TakTak Development"\n`;
  assert.deepEqual(identityHashes(out, B.toLowerCase()), [B]);
});

test('a relative CARGO_TARGET_DIR is resolved against the repository root', () => {
  const root = path.resolve('/repo');
  assert.equal(targetDir(root, 'build/m4'), path.join(root, 'build', 'm4'));
  assert.equal(targetDir(root, '/abs/target'), path.resolve('/abs/target'));
  assert.equal(targetDir(root, undefined), path.join(root, 'target'));
  assert.equal(targetDir(root, ''), path.join(root, 'target'));
});
