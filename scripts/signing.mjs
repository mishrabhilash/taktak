// Pure helpers for scripts/app.mjs, kept apart so they can be tested without building anything:
//   node --test scripts/*.test.mjs

import path from 'node:path';

/**
 * The distinct SHA-1 hashes of the code signing identities named `identity` in the output of
 * `security find-identity [-v] -p codesigning`, in listing order. `identity` may also be a
 * 40-hex SHA-1 hash, which then matches by hash. The same certificate listed twice (it is in two
 * keychains) counts once: codesign tolerates that, but not two different certificates with the
 * same name.
 *
 * Lines look like `  1) 0123…ABCD "TakTak Development"`; the summary line
 * (`1 valid identities found`) and, without `-v`, suffixes such as `(CSSMERR_TP_NOT_TRUSTED)`
 * after the name are allowed for.
 */
export function identityHashes(output, identity) {
  const byHash = /^[0-9a-f]{40}$/i.test(identity);
  const hashes = [];
  for (const line of output.split('\n')) {
    const match = /^\s*\d+\)\s+([0-9A-Fa-f]{40})\s+"(.*)"(?:\s+\(.*\))?\s*$/.exec(line);
    if (!match) continue;
    const [, hash, name] = match;
    const wanted = byHash ? hash.toUpperCase() === identity.toUpperCase() : name === identity;
    if (wanted && !hashes.includes(hash.toUpperCase())) hashes.push(hash.toUpperCase());
  }
  return hashes;
}

/**
 * The absolute target directory for a build started from the repository `root`: a relative
 * `CARGO_TARGET_DIR` is taken relative to `root`, where `npm run app` runs. The Tauri CLI runs
 * cargo from src-tauri, where cargo would resolve a relative value against src-tauri instead,
 * so app.mjs passes this absolute path on to `tauri build`, and both agree.
 */
export function targetDir(root, cargoTargetDir) {
  return cargoTargetDir ? path.resolve(root, cargoTargetDir) : path.join(root, 'target');
}
