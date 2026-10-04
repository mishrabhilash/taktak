// Pure helpers for scripts/notices.mjs, kept apart so they can be tested without cargo, vite or
// the file system:   node --test scripts/*.test.mjs

/** Top-level file names that hold a license, a copyright notice or an Apache NOTICE. */
const LICENSE_FILE = /^(licen[cs]e|copying|copyright|notice|unlicense|mit|apache)([-._ ].*)?$/i;
/** Files next to them that are not license texts. */
const NOT_LICENSE = /\.(rs|toml|json|lock|ya?ml|sh|py|js|ts|c|h)$/i;
/** Folders that hold one license file per license (REUSE layout and similar). */
export const LICENSE_DIR = /^licen[cs]es?$/i;

/** Whether a top-level file named `name` is a license text worth reproducing. */
export function isLicenseFile(name) {
  return LICENSE_FILE.test(name) && !NOT_LICENSE.test(name);
}

/**
 * The packages reachable from `rootIds` through normal (not dev, not build) dependency edges
 * of `cargo metadata`'s `resolve.nodes`, for any target platform: what the shipped binary can
 * link on some OS. Returns a Set of package ids, the roots included.
 */
export function shippedCrates(nodes, rootIds) {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const seen = new Set();
  const stack = [...rootIds];
  while (stack.length > 0) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of byId.get(id)?.deps ?? []) {
      // dep_kinds: [{ kind: null | "dev" | "build", target: null | "cfg(...)" }]
      if ((dep.dep_kinds ?? []).some((k) => k.kind === null)) stack.push(dep.pkg);
    }
  }
  return seen;
}

/**
 * The npm package (`name`, or `@scope/name`) a bundled module id belongs to, and the path of
 * that package's folder, or null for modules outside node_modules. The innermost
 * `node_modules/` wins, so nested copies are told apart.
 */
export function npmPackageOf(moduleId) {
  const id = moduleId.replace(/^\0/, '').split('?')[0].replaceAll('\\', '/');
  const marker = '/node_modules/';
  const at = id.lastIndexOf(marker);
  if (at < 0) return null;
  const rest = id.slice(at + marker.length).split('/');
  const name = rest[0].startsWith('@') ? `${rest[0]}/${rest[1]}` : rest[0];
  if (!name || name.endsWith('/undefined')) return null;
  return { name, dir: id.slice(0, at + marker.length) + name };
}

/** A license expression for display: SPDX-style `/` separators spelled as OR. */
export function normalizeLicense(expr) {
  if (!expr) return 'unknown';
  return expr
    .split(/\s*\/\s*/)
    .join(' OR ')
    .replace(/\s+/g, ' ')
    .trim();
}

/** License text normalized so that identical licenses compare equal (line ends, edges). */
export function normalizeText(text) {
  return text
    .replace(/^﻿/, '')
    .replace(/\r\n?/g, '\n')
    .split('\n')
    .map((line) => line.replace(/\s+$/, ''))
    .join('\n')
    .replace(/^\n+/, '')
    .replace(/\n+$/, '');
}

const IS_MIT = /permission is hereby granted, free of charge/i;
const IS_APACHE = (t) =>
  /apache license/i.test(t) && /version 2\.0/i.test(t) && /terms and conditions for use/i.test(t);

/**
 * The license texts to reproduce for a package under `license`. A package offered under a
 * choice of licenses that includes MIT (`MIT OR Apache-2.0`, …) is used under MIT, so when it
 * ships an MIT text, its full Apache-2.0 license text is left out; everything else it ships
 * (NOTICE files, other notices) is kept. Any other expression keeps every text.
 */
export function chooseTexts(license, texts) {
  const choices = license.replace(/[()]/g, ' ').split(/\s+OR\s+/).map((c) => c.trim());
  const pureChoice = !/\sAND\s/.test(license) && choices.length > 1;
  if (!pureChoice || !choices.includes('MIT')) return texts;
  if (!texts.some((t) => IS_MIT.test(t.text))) return texts;
  return texts.filter((t) => IS_MIT.test(t.text) || !IS_APACHE(t.text));
}

/**
 * The key under which license texts count as the same: letters and digits only, lower case.
 * Copies of one license that differ only in layout, line wrapping, punctuation or Markdown
 * markup share a key, while a different copyright holder or year does not.
 */
export function textKey(text) {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, ' ')
    .trim();
}

/** A Markdown code fence longer than any backtick run inside `text`. */
export function fenceFor(text) {
  const longest = Math.max(0, ...[...text.matchAll(/`+/g)].map((m) => m[0].length));
  return '`'.repeat(Math.max(3, longest + 1));
}

/** Text safe inside a Markdown table cell. */
export function cell(text) {
  return String(text ?? '')
    .replace(/\|/g, '\\|')
    .replace(/[\r\n]+/g, ' ')
    .trim();
}

/** A repository or homepage URL worth linking, or ''. Only http(s) URLs are kept. */
export function sourceUrl(pkg) {
  const raw = typeof pkg.repository === 'object' && pkg.repository ? pkg.repository.url : pkg.repository;
  const url = String(raw || pkg.homepage || '')
    .replace(/^git\+/, '')
    .replace(/^git:\/\//, 'https://')
    .replace(/\.git$/, '');
  if (/^github:/.test(url)) return `https://github.com/${url.slice('github:'.length)}`;
  if (/^[\w.-]+\/[\w.-]+$/.test(url)) return `https://github.com/${url}`;
  return /^https?:\/\//.test(url) ? url : '';
}

const byNameVersion = (a, b) =>
  a.name.localeCompare(b.name, 'en') || a.version.localeCompare(b.version, 'en');

/**
 * Renders THIRD_PARTY_NOTICES.md. `rust` and `npm` are lists of
 * `{ name, version, license, url, texts: [{ file, text }] }`. Identical license texts are
 * printed once, with every package that ships them. The output depends only on its input, so
 * the file is the same on every machine.
 */
export function renderNotices({ rust, npm }) {
  const groups = new Map(); // textKey -> { text, users: Set<string>, files: Set<string> }
  const withoutText = [];
  const label = (p) => `${p.name} ${p.version}`;
  for (const pkg of [...rust, ...npm]) {
    const texts = chooseTexts(
      pkg.license,
      pkg.texts.filter((t) => normalizeText(t.text).length > 0),
    );
    if (texts.length === 0) withoutText.push(pkg);
    for (const { file, text } of texts) {
      const normalized = normalizeText(text);
      const key = textKey(normalized);
      if (!groups.has(key)) groups.set(key, { text: normalized, users: new Set(), files: new Set() });
      const group = groups.get(key);
      // Print the same copy whatever order the packages come in: the smallest one.
      if (normalized < group.text) group.text = normalized;
      group.users.add(label(pkg));
      group.files.add(file);
    }
  }
  const ordered = [...groups.values()]
    .map((g) => ({ ...g, users: [...g.users].sort(), files: [...g.files].sort() }))
    .sort(
      (a, b) =>
        a.users[0].localeCompare(b.users[0], 'en') ||
        a.files[0].localeCompare(b.files[0], 'en') ||
        a.text.localeCompare(b.text, 'en'),
    );

  const table = (pkgs) => [
    '| Package | Version | License | Source |',
    '|---|---|---|---|',
    ...[...pkgs]
      .sort(byNameVersion)
      .map((p) => `| ${cell(p.name)} | ${cell(p.version)} | ${cell(p.license)} | ${p.url ? `<${p.url}>` : ''} |`),
  ];

  const out = [
    '# Third-party notices',
    '',
    '<!-- Generated by `node scripts/notices.mjs` (npm run notices). Do not edit by hand. -->',
    '',
    "TakTak's own code is MIT-licensed (see [`LICENSE`](LICENSE)). The sound packs it ships",
    'carry their own licenses, listed in [`CREDITS.md`](CREDITS.md). This file lists the',
    'third-party software compiled into the TakTak app, with the license texts and notices',
    'those packages ship:',
    '',
    `- **${rust.length} Rust crates**: every crate the \`taktak\` binary depends on through normal`,
    '  dependencies on macOS (Apple silicon and Intel), Windows x64 or Linux x64 (from',
    '  `Cargo.lock`, via `cargo metadata`). Each OS uses a subset of them.',
    '  Build tools, build scripts and test-only dependencies are not part of the app and are not',
    '  listed.',
    `- **${npm.length} npm packages**: the packages whose code Vite bundles into the app's user`,
    "  interface (from the production build's module graph and `package-lock.json`).",
    '',
    "Each package's license applies to that package only. Where a package offers a choice of",
    'licenses that includes MIT (`MIT OR Apache-2.0`, …), TakTak uses it under MIT, and only its',
    'MIT text is reproduced here. Source code for every package is available from the link',
    'given, and from crates.io or the npm registry at the exact version listed.',
    '',
    '## Rust crates',
    '',
    ...table(rust),
    '',
    '## npm packages bundled into the user interface',
    '',
    ...table(npm),
    '',
  ];
  if (withoutText.length > 0) {
    out.push(
      '## Packages that ship no license file',
      '',
      'These packages declare their license in their manifest but publish no license text. The',
      'standard text of the license named applies; copies of those licenses are reproduced below',
      'for other packages under them.',
      '',
      ...[...withoutText]
        .sort(byNameVersion)
        .map((p) => `- ${label(p)}: ${p.license}${p.url ? ` (<${p.url}>)` : ''}`),
      '',
    );
  }
  out.push('## License texts and notices', '');
  ordered.forEach((g, i) => {
    const fence = fenceFor(g.text);
    out.push(
      `### Notice ${i + 1}`,
      '',
      `Used by: ${g.users.join(', ')}. Shipped as ${g.files.map((f) => `\`${f}\``).join(', ')}.`,
      '',
      `${fence}text`,
      g.text,
      fence,
      '',
    );
  });
  return `${out.join('\n').replace(/\n+$/, '')}\n`;
}
