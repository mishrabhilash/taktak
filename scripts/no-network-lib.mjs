// Pure rules for scripts/no-network.mjs, kept apart so they can be tested without cargo or a
// build:   node --test scripts/*.test.mjs
//
// TakTak never uses the internet. These lists are what CI holds every change to.

/** Crates that exist to talk to the network (HTTP, TLS, WebSocket, DNS, QUIC, servers). */
export const DENIED_CRATES = [
  'reqwest', 'hyper', 'hyper-util', 'hyper-tls', 'hyper-rustls', 'h2', 'h3', 'ureq', 'isahc',
  'surf', 'attohttpc', 'minreq', 'curl', 'curl-sys', 'openssl', 'openssl-sys', 'native-tls',
  'rustls', 'tokio-rustls', 'tokio-native-tls', 'async-tls', 'webpki', 'rustls-webpki',
  'tungstenite', 'tokio-tungstenite', 'async-tungstenite', 'websocket', 'ws', 'quinn',
  'trust-dns-resolver', 'hickory-resolver', 'tiny_http', 'warp', 'axum', 'actix-web',
  'rocket', 'tide', 'libssh2-sys', 'git2', 'oauth2', 'tauri-plugin-http', 'tauri-plugin-updater',
  'tauri-plugin-websocket', 'tauri-plugin-upload', 'tauri-plugin-deep-link', 'sentry',
];

/**
 * Denied crates that may appear in Cargo.lock, with the reason. Cargo.lock lists the crates of
 * every target, including mobile ones TakTak never builds; these must still never reach a
 * desktop build, which the dependency-tree check enforces separately (it has no allow-list).
 */
export const LOCK_ALLOWED = {
  reqwest:
    "tauri's dev-server proxy, a dependency only for Android and iOS targets (cfg(any(target_os = \"android\", ...))), never built for desktop",
  hyper: 'pulled in by reqwest above (Android and iOS only)',
  'hyper-util': 'pulled in by reqwest above (Android and iOS only)',
};

/** npm packages that exist to talk to the network; none may be a dependency of the UI. */
export const DENIED_NPM = [
  'axios', 'node-fetch', 'cross-fetch', 'isomorphic-fetch', 'ky', 'got', 'superagent',
  'socket.io-client', 'ws', 'undici', '@sentry/browser', '@sentry/svelte',
  '@tauri-apps/plugin-http', '@tauri-apps/plugin-updater', '@tauri-apps/plugin-websocket',
  '@tauri-apps/plugin-upload',
];

/** Web APIs that reach the network, which TakTak's UI source must not call. */
export const DENIED_WEB_APIS =
  /\bfetch\s*\(|\bsendBeacon\s*\(|\bnew\s+(XMLHttpRequest|WebSocket|EventSource|RTCPeerConnection|WebTransport)\b/;

/** Rust networking from the standard library, which TakTak's crates must not use. */
export const DENIED_RUST_APIS = /\bstd::net\b|\b(TcpStream|TcpListener|UdpSocket|ToSocketAddrs)\b/;

/**
 * URLs allowed to appear as text in the shipped UI bundle. None of them is ever requested (the
 * CSP's connect-src allows only Tauri's IPC); they are identifiers and documentation links.
 */
export const ALLOWED_URLS = [
  [/^http:\/\/www\.w3\.org\//, 'XML namespace names (SVG, XHTML), never fetched'],
  [/^https:\/\/svelte\.dev\/e\//, "links in Svelte's runtime error messages"],
  [/^https:\/\/taktak\.tech(\/|$)/, 'the project website (docs link)'],
  [/^https:\/\/github\.com\/<?mishrabhilash>?\/taktak(\/|$)/, 'the project repository (docs link)'],
  [/^https:\/\/creativecommons\.org\/(licenses|publicdomain)\//, 'license links'],
  [/^https:\/\/(opensource\.org|spdx\.org)\/licenses\//, 'license links'],
];

/** The package names in a Cargo.lock, sorted and unique. */
export function lockPackages(lockText) {
  const names = new Set();
  for (const match of lockText.matchAll(/^\[\[package\]\]\s*\nname = "([^"]+)"/gm)) {
    names.add(match[1]);
  }
  return [...names].sort();
}

/** The crate names in `cargo tree --prefix none` output (first word of each line). */
export function treeCrates(treeText) {
  const names = new Set();
  for (const line of treeText.split('\n')) {
    const name = line.trim().split(/\s+/)[0];
    if (name && /^[A-Za-z0-9_-]+$/.test(name)) names.add(name);
  }
  return [...names].sort();
}

/** Denied crates in `names`, minus `allowed` (an object of name -> reason). */
export function deniedIn(names, denied, allowed = {}) {
  const set = new Set(denied);
  return names.filter((n) => set.has(n) && !Object.hasOwn(allowed, n));
}

/** The http(s) URLs in `text`, unique, in order of appearance. */
export function urlsIn(text) {
  const urls = new Set();
  for (const match of text.matchAll(/https?:\/\/[^\s"'`<>()\\{}|^]+/g)) {
    urls.add(match[0].replace(/[.,;:]+$/, ''));
  }
  return [...urls];
}

/** The URLs in `urls` that no ALLOWED_URLS pattern matches. */
export function disallowedUrls(urls) {
  return urls.filter((u) => !ALLOWED_URLS.some(([pattern]) => pattern.test(u)));
}

/**
 * Problems with a Tauri config: a connect-src that reaches beyond Tauri's own IPC, or a
 * plugin configuration for networking plugins.
 */
export function configProblems(config) {
  const problems = [];
  const csp = config?.app?.security?.csp;
  const connect = typeof csp === 'object' && csp ? csp['connect-src'] : undefined;
  if (typeof csp === 'string' || connect === undefined) {
    problems.push('app.security.csp must be an object with an explicit connect-src');
  } else {
    const allowed = new Set(["'self'", 'ipc:', 'http://ipc.localhost']);
    for (const source of String(connect).split(/\s+/).filter(Boolean)) {
      if (!allowed.has(source)) problems.push(`connect-src allows ${source}`);
    }
  }
  for (const plugin of ['updater', 'http', 'websocket', 'upload']) {
    if (config?.plugins?.[plugin]) problems.push(`plugins.${plugin} is configured`);
  }
  if (config?.bundle?.createUpdaterArtifacts) problems.push('bundle.createUpdaterArtifacts is set');
  return problems;
}
