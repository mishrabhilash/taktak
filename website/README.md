# taktak.tech

The TakTak landing page: one static page, plain HTML, CSS and JavaScript. No build step, no
dependencies, no trackers, and no requests to anyone else's servers (fonts are the system's,
icons are inline SVG, and a strict Content-Security-Policy allows only this site's own files).

```
website/
├── index.html            the page (CSP in a <meta> tag)
├── styles.css            light and dark mode, responsive, reduced motion
├── main.js               CONFIG (repo URL, version), OS-detecting download button, sound demo
├── assets/
│   ├── sounds/<pack>/    preview.mp3, keys.mp3 + keys.json, LICENSE.txt where the pack has one
│   ├── og-image.svg      social preview source
│   └── og-image.png      social preview (1200×630)
├── favicon.svg, favicon-32.png, apple-touch-icon.png
├── _headers              Cloudflare Pages headers (CSP, nosniff, no referrer, caching)
├── CNAME                 taktak.tech, for GitHub Pages
├── 404.html, robots.txt, sitemap.xml
└── tools/build_sounds.py optional: regenerates assets/sounds and the pack cards
```

## Preview locally

```sh
cd website
python3 -m http.server 8787
# open http://localhost:8787
```

Any static file server works. Opening `index.html` straight from disk (`file://`) shows the
page, but the sound demo needs `http://` because browsers block `fetch` from `file://`.

## Change the repository URL or the release version

Both live in **one constant**, `CONFIG` at the top of [`main.js`](main.js):

```js
const CONFIG = {
  repo: 'https://github.com/mishrabhilash/taktak',
  version: '0.1.0',
};
```

Every GitHub link on the page is an `<a data-repo="/path">` that the script points at
`CONFIG.repo + path`, and the download buttons point at
`<repo>/releases/download/v<version>/<file>`, using the file names the release workflow
produces (`TakTak_<version>_universal.dmg`, `_x64_en-US.msi`, `_x64-setup.exe`,
`_amd64.AppImage`, `_amd64.deb`). Bump `version` with each release. Without JavaScript the
links fall back to the matching section of the page.

## The sounds

`assets/sounds/` is generated from the bundled packs in [`packs/`](../packs) and committed, so
deploying needs nothing but the files. After a pack changes, regenerate it (needs Python 3.9+
and ffmpeg with libmp3lame):

```sh
python3 website/tools/build_sounds.py --ffmpeg /opt/homebrew/bin/ffmpeg
```

It converts each pack's `preview.wav` to a small mono MP3, and puts every sound the "type here"
box can play into one MP3 per pack (`keys.mp3`), with `keys.json` saying where each sound
starts and which sound each key plays on press and on release. Which sound a key plays is
resolved exactly like the app does (the same fallback chain and the same fixed hash of the key
name, `consistent_index` in `src-tauri/core/src/audio/mixer.rs`), so a key sounds the same here
as in TakTak. It also rewrites the pack `<option>`s and cards in `index.html` between the
`packs:options` and `packs:cards` markers, with each pack's license badge and the credit its
license asks for (CC BY packs show their full attribution, source and license URI), and copies
each pack's `LICENSE.txt` next to its sounds.

Audio loads only when used: a preview on its first Play, a pack's keystrokes when the type box
gets focus or another pack is picked.

## Social preview image

`assets/og-image.png` is rendered from `assets/og-image.svg` with headless Chrome:

```sh
cd website
cat > /tmp/og.html <<EOF
<!doctype html><style>html,body{margin:0}img{display:block}</style>
<img src="file://$PWD/assets/og-image.svg" width="1200" height="630">
EOF
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --hide-scrollbars \
  --allow-file-access-from-files --window-size=1200,630 --screenshot="$PWD/assets/og-image.png" file:///tmp/og.html
```

## Deploy

### Cloudflare Pages

1. Workers & Pages → Create → Pages → Connect to Git, and pick this repository.
2. Build settings: framework preset **None**, build command **empty**, build output directory
   **`website`**, production branch `main`.
3. Custom domains → add `taktak.tech` (and `www.taktak.tech` if you want it, redirected to the
   apex with a Bulk Redirect or a Page Rule).

`_headers` is applied automatically: the same CSP as the `<meta>` tag plus `frame-ancestors
'none'`, `X-Content-Type-Options`, `Referrer-Policy: no-referrer`, a restrictive
`Permissions-Policy`, HSTS and a one-day cache for `assets/`. `README.md` and `tools/` are
deployed too (harmless; `robots.txt` keeps `tools/` out of search engines). To leave them out,
use the build command `mkdir -p _site && cp -R website/. _site/ && rm -rf _site/tools _site/README.md`
with output directory `_site`.

### GitHub Pages

[`.github/workflows/pages.yml`](../.github/workflows/pages.yml) deploys `website/` (without
`tools/` and this README) on every push to `main` that touches it, and can be run by hand.

1. Settings → Pages → Build and deployment → Source: **GitHub Actions**.
2. Push to `main` (or run the "Pages" workflow).
3. Settings → Pages → Custom domain: `taktak.tech`, then tick **Enforce HTTPS** once the
   certificate is issued.

`CNAME` holds `taktak.tech`, but the custom domain only works once DNS points at GitHub
Pages: `A` records for the apex to `185.199.108.153`, `185.199.109.153`, `185.199.110.153`
and `185.199.111.153` (and `AAAA` to `2606:50c0:8000::153` … `8003::153`), plus a `CNAME`
for `www` to `mishrabhilash.github.io`. Until then the site is served at
`https://mishrabhilash.github.io/taktak/`, where everything works except `404.html` (it uses
root-relative paths). GitHub Pages ignores `_headers`; the `<meta>` CSP still applies.

## Rules for changes

- No third-party requests of any kind: no CDNs, web fonts, analytics, embeds, iframes or
  remote images. Links to GitHub and taktak.tech are fine. If you add a resource, keep it in
  this folder; the CSP will block anything else (keep the `<meta>` CSP and `_headers` in sync).
- No inline scripts, inline styles or `style=""` attributes (the CSP blocks them).
- Keep the promise word for word: "Fully offline — TakTak never uses the internet."
- Keep the page light: everything a visitor can load, all audio included, is under 1 MB today.

## TODO

- Real repository URL in `CONFIG.repo` (`main.js`).
- A demo video or GIF in the hero, replacing the keyboard illustration (see the `TODO(demo
  video)` comment in `index.html`); self-host it in `assets/`.
- Point DNS for taktak.tech at Cloudflare Pages or GitHub Pages.
