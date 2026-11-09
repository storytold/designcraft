# Hosting DesignCraft for the web

`designcraft-web-<version>.zip` (from the GitHub release, or `packaging/web/package.sh`) holds a
static site in `designcraft-web-<version>/`:

| File | What it is |
|---|---|
| `index.html` | The page. It loads everything through relative URLs. |
| `designcraft-web-<hash>.js` | wasm-bindgen glue (generated, ES module) |
| `designcraft-web-<hash>_bg.wasm` | The app, about 13 MB, or 5 MB with compression |
| `_headers`, `.htaccess` | Sample header rules for Netlify/Cloudflare Pages and Apache |

There is no server-side code. Upload the folder's contents anywhere that serves static files.

## Any path works

All URLs in `index.html` are relative (`public_url = "./"` in `apps/designcraft-web/Trunk.toml`),
so the site works at a domain root (`https://example.com/`), under a prefix
(`https://example.com/tools/designcraft/`) and from a CDN bucket. The asset names carry a content
hash, so they can be cached forever. Only `index.html` needs revalidation.

## Required server settings

- **MIME type:** serve `.wasm` as `application/wasm`. Browsers refuse to stream-compile it under
  any other type, and the app then loads slowly or not at all. Serve `.js` as `text/javascript`.
  Most hosts already do both. For nginx, check that `mime.types` has `application/wasm wasm;`.
- **Compression:** turn on gzip or Brotli for `.wasm`, `.js` and `.html`. That takes the
  download from about 13 MB to about 5 MB. You can also precompress (`brotli -k *.wasm`) and let
  the server send `Content-Encoding: br`.
- **Caching:** `Cache-Control: public, max-age=31536000, immutable` on the hashed `.wasm` and
  `.js` files, and `no-cache` on `index.html`.
- **HTTPS:** WebGPU (and the clipboard) only work in a secure context, which means `https://`
  or `http://localhost`. Over plain HTTP elsewhere, the app falls back to WebGL2.
- **No special isolation headers:** DesignCraft doesn't use `SharedArrayBuffer`, so it doesn't
  need `Cross-Origin-Opener-Policy` or `Cross-Origin-Embedder-Policy`. If your site already sends
  COEP `require-corp`, also send `Cross-Origin-Resource-Policy: same-origin` (or `cross-origin`
  when the files live on a CDN) on the app's files.

nginx example:

```nginx
location /designcraft/ {
    types { application/wasm wasm; text/javascript js; text/html html; }
    gzip on;
    gzip_types application/wasm text/javascript text/html;
    location ~* \.(wasm|js)$ { add_header Cache-Control "public, max-age=31536000, immutable"; }
    location ~* index\.html$ { add_header Cache-Control "no-cache"; }
}
```

Local test: `python3 -m http.server 8765` inside the folder, then open http://localhost:8765/.

## Embedding in a page (iframe)

```html
<iframe
  src="https://example.com/designcraft/"
  title="DesignCraft image editor"
  style="width: 100%; height: 720px; border: 0;"
  allow="fullscreen; clipboard-read; clipboard-write"
  allowfullscreen>
</iframe>
```

- The app fills the iframe and follows its size, so size the iframe and not the app.
- Keyboard shortcuts go to the iframe after the user clicks into it, as with any embedded app.
- **Cross-origin embeds** work. Preferences are kept in the iframe's `localStorage`. Browsers
  that partition or block third-party storage may forget them between visits, and the app
  then starts with defaults.
- **Sandboxed iframes** need at least
  `sandbox="allow-scripts allow-same-origin allow-downloads allow-popups"`. Without
  `allow-same-origin` there's no storage. Without `allow-downloads`, Save and Export (browser
  downloads) are blocked.
- Don't send `X-Frame-Options: DENY` or a `frame-ancestors` CSP that excludes the embedding page.

## Renderer selection and fallback flags

DesignCraft renders with wgpu. It uses **WebGPU** when the browser has it and falls back to
**WebGL2** on its own. URL query flags override this, and they work on the iframe `src` too:

| Flag | Effect |
|---|---|
| *(none)* | WebGPU if available, otherwise WebGL2 |
| `?webgl` | Force the WebGL2 backend (useful when a WebGPU driver misbehaves) |
| `?cpu` | Force the CPU canvas path (slowest, most compatible) |

For example: `<iframe src="https://example.com/designcraft/?webgl" ...>`.

A browser with neither WebGPU nor WebGL2 gets a message in place of the app.

## Driving it from the page (`window.designcraft`)

Every DesignCraft web build puts `window.designcraft` on the page, from [`craft-web-api`](https://github.com/storytold/craft-libs/tree/main/crates/web-api) (storytold/craft-libs#413), the same shape in every Craft app. `designcraft.request(method, params)` calls any method of the control channel the desktop app serves (`docs/control-protocol.md`), and `execute(command, params)`, `commands()`, `docInspect(params)`, `uiInspect(params)` and `screenshot(params)` are shortcuts for the common ones. Each returns a Promise that resolves with the method's result and rejects with an `Error`; a call the app does not answer within 30 seconds rejects. DesignCraft's control requests cannot be withdrawn, so a call that times out in the page still runs when the app reaches it, as it does for a desktop client that timed out.

```js
await designcraft.execute("file.new", {});                          // docInspect rejects until a document is open
const doc = await designcraft.docInspect();                          // pages, frames, selection
await designcraft.execute("frame.create", { rect: [36, 36, 300, 200], content: "text" });
const { data } = await designcraft.screenshot();                     // the window as PNG (base64)
```

The API is installed in every session. There is no file system in the page: a screenshot without `path` comes back inline.
