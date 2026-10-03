#!/usr/bin/env node
// Release build without the builder's file paths in the binary.
//
//   npm run release [-- <tauri build options>]   e.g. npm run release -- --bundles app
//   npm run release -- --check [<binary>…]      only check binaries (default: the release one)
//
// Compiled-in panic locations name source files by absolute path, including every dependency
// under ~/.cargo/registry, so a plain `tauri build` ships the builder's home directory (and
// account name). This runs `tauri build` with rustc's --remap-path-prefix for this checkout,
// the Cargo and rustup homes and the home directory, then fails if the home directory is still
// in the binary. Cargo's `trim-paths` profile setting would do the same, but it is not stable
// yet (Rust 1.99).

import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');
const home = homedir();

/** `--remap-path-prefix` pairs. rustc applies the last one that matches: most specific last. */
function remaps() {
  return [
    [home, '/home'],
    [process.env.RUSTUP_HOME || path.join(home, '.rustup'), '/rustup'],
    [process.env.CARGO_HOME || path.join(home, '.cargo'), '/cargo'],
    [root, '/taktak'],
  ];
}

/** The environment for cargo: the caller's flags plus the remaps, spaces in paths allowed. */
function buildEnv() {
  const env = { ...process.env };
  const existing = env.CARGO_ENCODED_RUSTFLAGS
    ? env.CARGO_ENCODED_RUSTFLAGS.split('\x1f')
    : (env.RUSTFLAGS ?? '').split(' ').filter(Boolean);
  const flags = remaps().map(([from, to]) => `--remap-path-prefix=${from}=${to}`);
  // CARGO_ENCODED_RUSTFLAGS takes precedence over RUSTFLAGS and is split on 0x1f, not spaces.
  env.CARGO_ENCODED_RUSTFLAGS = [...existing, ...flags].join('\x1f');
  delete env.RUSTFLAGS;
  return env;
}

/** The app binary `tauri build <args>` produces. */
function builtBinary(args) {
  const targetDir = process.env.CARGO_TARGET_DIR
    ? path.resolve(root, process.env.CARGO_TARGET_DIR)
    : path.join(root, 'target');
  const i = args.findIndex((a) => a === '--target' || a === '-t');
  const triple = i >= 0 ? args[i + 1] : undefined;
  const profile = args.includes('--debug') || args.includes('-d') ? 'debug' : 'release';
  const exe = process.platform === 'win32' ? 'taktak.exe' : 'taktak';
  return path.join(targetDir, ...(triple ? [triple] : []), profile, exe);
}

/** How many times the home directory's path occurs in `file`. */
function countHome(file) {
  const data = readFileSync(file);
  const spellings = new Set([home, home.replaceAll('\\', '/')]);
  let count = 0;
  for (const needle of [...spellings].map((h) => Buffer.from(h))) {
    for (let at = data.indexOf(needle); at >= 0; at = data.indexOf(needle, at + needle.length)) {
      count++;
    }
  }
  return count;
}

/** Checks each binary; true when none contains the home directory. */
function check(files) {
  let clean = true;
  for (const file of files) {
    if (!existsSync(file)) {
      console.error(`release: ${file} does not exist`);
      clean = false;
      continue;
    }
    const count = countHome(file);
    const shown = file.startsWith(root + path.sep) ? path.relative(root, file) : file;
    if (count === 0) {
      console.log(`release: ${shown} holds no paths under the home directory`);
    } else {
      console.error(`release: ${shown} still holds the home directory path ${count} times`);
      clean = false;
    }
  }
  return clean;
}

const args = process.argv.slice(2);
if (args[0] === '--check') {
  const files = args.length > 1 ? args.slice(1).map((f) => path.resolve(f)) : [builtBinary([])];
  process.exit(check(files) ? 0 : 1);
}

const cli = path.join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
const build = spawnSync(process.execPath, [cli, 'build', ...args], {
  cwd: root,
  env: buildEnv(),
  stdio: 'inherit',
});
if (build.status !== 0) process.exit(build.status ?? 1);
process.exit(check([builtBinary(args)]) ? 0 : 1);
