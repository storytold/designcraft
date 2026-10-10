# Control protocol

Start the app with `--control <port>` (or `DESIGNCRAFT_CONTROL_PORT`). The server listens on
`127.0.0.1` only and speaks JSON lines: one request object per line, one reply per line.

**Only requests are read.** Every line must be a JSON object with a string `method` (`id` and `params`
are optional; blank lines are skipped). Anything else gets one error reply
(`{"ok": false, "error": "… closing the connection"}`) and the server **closes the connection**, so
nothing sent after it on that connection runs. That covers text that isn't JSON, a JSON array or number,
an object without `method`, invalid UTF-8, and a line longer than 4 MiB. An HTTP request (for example a
web page's cross-origin `fetch` to `127.0.0.1:<port>`) therefore can't smuggle a command in its body:
its request line is rejected first. At most 16 connections are served at once; further ones get an error
line and are closed. Clients that get an error reply should reconnect. The port has no authentication,
so only enable it while you use it. Transport: `apps/designcraft/src/control_server.rs`.

```json
{"id": 1, "method": "engine.execute", "params": {"command": "frame.create", "params": {"rect": [36, 36, 300, 200], "content": "text"}}}
{"id": 1, "ok": true, "result": {"id": 10, "story": 11}}
```

| Method | Params | What it does |
|---|---|---|
| `engine.execute` / `ui.menu.invoke` | `{command, params}` | Run any engine or UI command (see `engine.commands`). `ui.menu.invoke` without `params` acts like choosing the menu item: a command labelled "…" that takes parameters opens its dialog |
| `engine.commands` | — | Every command: id, label, menu path, shortcut, params doc, enabled |
| `document.inspect` | — | Pages, spreads, items, stories (overset), styles, swatches, selection |
| `ui.inspect` | — | Tool, UI state, view (zoom/origin), canvas rect, perf |
| `ui.menu.list` / `ui.tool.list` | — | Menu tree / Tools panel groups |
| `ui.tool.select` | `{tool}` | Select a tool (`selection`, `type`, `rectangleFrame`, …) |
| `ui.pointer` | `{events:[{kind: down\|drag\|up\|move\|doubleclick, x, y, space?: "screen"\|"canvas", clicks?}], mods?}` | Drive the active tool through the same code path as the mouse; `clicks` on a `down` is the press count (3 = triple click); returns the selection, the tool and its cursor at the last event (e.g. `LoadedText`, `ThreadLink`, `Unthread`) |
| `ui.key` / `ui.text` | `{key, shift?, alt?, cmd?}` / `{text}` | Synthetic keyboard input (typing into a text frame) |
| `ui.move` / `ui.click` / `ui.drag` | screen points, `button?: left\|right\|middle` | Real egui pointer input — reaches every widget, menu and panel |
| `ui.set` | `{brightness?, panel?, rulers?, guides?, frameEdges?, baselineGrid?, textThreads?, screenMode?, zoom?, page?, fit?}` | UI state |
| `ui.dialog.open` | `{id, fields?}` | Open a dialog by id (e.g. `paragraphStyleOptions` with `{name, section}`) |
| `ui.dialog.set` / `ui.dialog.confirm` / `ui.dialog.cancel` | `{field, value}` | Fill and confirm the open dialog |
| `ui.resize`, `ui.focus` | | Window control |
| `ui.screenshot` | `{path?}` | PNG of the whole window |
| `ui.render` | `{path?, page?, scale?, bleed?}` | Render a page headlessly (PNG; base64 if no path) |
| `app.open` / `app.save` / `app.export` / `app.quit` | | Files |

Data merge runs through `engine.execute` like any command: `data.source.select`, `data.fields`, `data.placeholder.add` /
`.remove`, `data.options`, `data.preview` / `data.preview.stop`, `data.merge`. `data.merge` creates and activates a new
merged document; the template stays as it was ([agents.md](agents.md#data-merge)).

Headless window screenshots (locked screen, hidden window): `cargo run -p designcraft-ui-egui --example ui_shot -- script.jsonl`, where each line is one of the requests above, `{"shot": "/abs/out.png"}` or `{"steps": n}` (renders the whole UI offscreen with wgpu).

The MCP server (`designcraft-cli mcp`) wraps the same methods for Claude and other agents.
