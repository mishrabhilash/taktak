#!/usr/bin/env node
// Local .app build with a stable code signature, so macOS keeps TakTak's Input Monitoring
// permission across rebuilds.
//
//   npm run app [-- <tauri build options>]        e.g. npm run app -- --debug
//
// macOS ties Input Monitoring to the app's code identity. The linker's ad-hoc signature changes
// with every build, so each rebuild loses the permission. Signed with the same certificate
// every time, the identity stays the same and the permission survives.
//
// This runs `tauri build --bundles app`, then signs the bundle with the code signing identity
// "TakTak Development" when `security find-identity -v -p codesigning` lists it, or ad hoc
// otherwise (and says how to create the certificate). Signing happens afterwards with
// `codesign` (no secure timestamp, so it works offline and with a self-signed certificate).
// See docs/app.md § Development signing. Not for distribution: that needs a Developer ID
// signature and notarization (`npm run release`).
//
// Set TAKTAK_SIGNING_IDENTITY to use another identity (a name or a SHA-1 hash). When two
// different certificates share the name, the first is used and the duplicate is pointed out.
// CARGO_TARGET_DIR is honoured; a relative one is taken relative to the repository root.

import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { identityHashes, targetDir } from './signing.mjs';

const root = path.resolve(import.meta.dirname, '..');
const identity = process.env.TAKTAK_SIGNING_IDENTITY || 'TakTak Development';
const args = process.argv.slice(2);

/** Runs `command`, returning its status and output (stdout and stderr captured). */
function capture(command, commandArgs) {
  const result = spawnSync(command, commandArgs, { encoding: 'utf8' });
  return { status: result.status ?? 1, out: `${result.stdout ?? ''}${result.stderr ?? ''}` };
}

/** Runs `command` with its output shown; exits on failure. */
function run(command, commandArgs, options = {}) {
  const result = spawnSync(command, commandArgs, { cwd: root, stdio: 'inherit', ...options });
  if (result.status !== 0) {
    console.error(`app: ${command} ${commandArgs.join(' ')} failed`);
    process.exit(result.status ?? 1);
  }
}

/**
 * Whether `identity` is a code signing identity: `valid` (usable as is; `hashes` are the SHA-1s
 * of the matching certificates, more than one when several share the name), `untrusted` (found,
 * but macOS does not trust the self-signed certificate for code signing yet) or `missing`.
 */
function identityState() {
  const valid = identityHashes(
    capture('security', ['find-identity', '-v', '-p', 'codesigning']).out,
    identity,
  );
  if (valid.length > 0) return { state: 'valid', hashes: valid };
  const all = capture('security', ['find-identity', '-p', 'codesigning']).out;
  return { state: identityHashes(all, identity).length > 0 ? 'untrusted' : 'missing', hashes: [] };
}

/** The .app bundle `tauri build <args>` produces in `target`. */
function bundlePath(target) {
  const i = args.findIndex((a) => a === '--target' || a === '-t');
  const triple = i >= 0 ? args[i + 1] : undefined;
  const profile = args.includes('--debug') || args.includes('-d') ? 'debug' : 'release';
  return path.join(target, ...(triple ? [triple] : []), profile, 'bundle', 'macos', 'TakTak.app');
}

const cli = path.join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
const bundles = args.includes('--bundles') || args.includes('-b') ? [] : ['--bundles', 'app'];
// Absolute, so cargo (run by tauri from src-tauri) and bundlePath agree on where it is.
const target = targetDir(root, process.env.CARGO_TARGET_DIR);
const env = process.env.CARGO_TARGET_DIR
  ? { ...process.env, CARGO_TARGET_DIR: target }
  : process.env;
run(process.execPath, [cli, 'build', ...bundles, ...args], { env });

if (process.platform !== 'darwin') {
  console.log('app: built; signing only matters on macOS, so the bundle is left as it is.');
  process.exit(0);
}

const app = bundlePath(target);
if (!existsSync(app)) {
  console.error(`app: ${app} was not built`);
  process.exit(1);
}

const { state, hashes } = identityState();
if (state === 'valid') {
  // By hash: codesign refuses a name that matches two different certificates.
  const [hash, ...others] = hashes;
  if (others.length > 0) {
    console.log(
      `app: ${hashes.length} different "${identity}" certificates found (${hashes.join(', ')});\n` +
        `     signing with the first, ${hash}. Delete the others in Keychain Access (login\n` +
        '     keychain, My Certificates) so every build keeps the same identity.',
    );
  }
  run('codesign', ['--force', '--options', 'runtime', '--sign', hash, app]);
  console.log(`app: signed with "${identity}": Input Monitoring stays granted across rebuilds.`);
} else {
  run('codesign', ['--force', '--options', 'runtime', '--sign', '-', app]);
  console.log('app: signed ad hoc: macOS will ask for Input Monitoring again after every rebuild.');
  if (state === 'untrusted') {
    console.log(
      `app: "${identity}" is in your keychain but not trusted for code signing. In Keychain\n` +
        '     Access, open the certificate, expand Trust and set Code Signing to Always Trust.',
    );
  } else {
    console.log(
      `app: to keep the permission, create a self-signed "${identity}" certificate once:\n` +
        '     Keychain Access → Certificate Assistant → Create a Certificate…, name\n' +
        `     "${identity}", Identity Type "Self-Signed Root", Certificate Type "Code Signing".\n` +
        '     Then set it to Always Trust for Code Signing. Details: docs/app.md.',
    );
  }
}
run('codesign', ['--verify', '--strict', app]);
console.log(`app: ${app.startsWith(root + path.sep) ? path.relative(root, app) : app}`);
console.log(
  'app: after switching signatures, reset the old permission once:\n' +
    '     tccutil reset ListenEvent tech.taktak.app',
);
