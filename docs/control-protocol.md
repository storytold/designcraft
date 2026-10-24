# Control protocol

Start the app with `--control <port>` (or `DESIGNCRAFT_CONTROL_PORT`). The server listens on
`127.0.0.1` only and speaks JSON lines: one request object per line, one reply per line.

For an isolated native test profile, set `DESIGNCRAFT_CONFIG_DIR` to a dedicated directory before
launch. UI/engine preferences and the `Recovery` directory then live under that root. An unset
override preserves the usual platform paths (or `DesignCraftData` next to the executable when a
`portable.txt` sits there); an explicitly empty override disables those paths.
`DESIGNCRAFT_NO_PREFS` still disables preference reads/writes independently of recovery.

Control pointer and key input is queued across native frames. After a drag, wait for both the
expected document state and `document.inspect.canUndo` before testing undo; an object can appear
in the preview before its undo entry is committed. On macOS, default shortcuts owned by the native
menu (including Command-Z) are skipped by the egui shortcut handler. `ui.key` does not send a Cocoa
menu event; use `ui.menu.invoke` for a menu-command test and test physical accelerators separately.

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
| `engine.execute` / `ui.menu.invoke` | `{command, params}` | Run any engine or UI command (see `engine.commands`). `ui.menu.invoke` without `params` acts like choosing the menu item: a command labelled "…" that takes parameters opens its dialog. A key the command's params description doesn't name is an error (`unknown parameter …`), not ignored |
| `engine.commands` | — | Every command: id, label, menu path, shortcut, params doc, enabled |
| `document.inspect` | — | Pages, spreads, items, stories (overset), styles, swatches, selection |
| `ui.inspect` | — | Tool, UI state, view (zoom/origin), canvas rect, panel layout (`docking`), perf |
| `ui.docking` | — | The panel layout in use, the focused panel and the docking hit rects (see Shared panel docking) |
| `ui.menu.list` / `ui.tool.list` | — | Menu tree / Tools panel groups |
| `ui.tool.select` | `{tool}` | Select a tool (`selection`, `type`, `rectangleFrame`, …) |
| `ui.pointer` | `{events:[{kind: down\|drag\|up\|move\|doubleclick, x, y, space?: "screen"\|"canvas", clicks?}], mods?}` | Drive the active tool through the same code path as the mouse; `clicks` on a `down` is the press count (3 = triple click); returns the selection, the tool and its cursor at the last event (e.g. `LoadedText`, `ThreadLink`, `Unthread`) |
| `ui.key` / `ui.text` | `{key, shift?, alt?, cmd?}` / `{text}` | Synthetic keyboard input (typing into a text frame) |
| `ui.ime` | `{preedit?, active?: [start, end], commit?}` | Input method events as the system IME sends them: marked text (shown in place, `active` is the clause being converted, in characters) and the committed text (typed as one `text.insert`) |
| `ui.paste` | `{text}` | What Ctrl/Cmd+V delivers from the system clipboard: text into the text being edited, or SVG markup (vector art copied in Illustrator, VectorCraft or Figma) as editable paths in the middle of the view |
| `ui.move` / `ui.click` / `ui.drag` | screen points, `button?: left\|right\|middle`, `count?: 1–255` (click/drag, default 1), `steps?: positive integer` (drag, default 8) | Real egui pointer input — reaches every widget, menu and panel |
| `ui.set` | `{brightness?, panel?, rulers?, guides?, frameEdges?, baselineGrid?, documentGrid?, textThreads?, screenMode?, zoom?, page?, fit?}` | UI state |
| `ui.dialog.open` | `{id, fields?}` | Open a dialog by id (e.g. `paragraphStyleOptions` or `characterStyleOptions` with `{name, section}`, or `{new: true}` for a new style) |
| `ui.dialog.set` / `ui.dialog.confirm` / `ui.dialog.cancel` | `{field, value}` | Fill and confirm the open dialog |
| `ui.resize`, `ui.focus` | | Window control |
| `ui.screenshot` | `{path?}` | PNG of the whole window |
| `ui.render` | `{path?, page?, scale?, bleed?}` | Render a page headlessly (PNG; base64 if no path) |
| `app.open` / `app.save` / `app.export` | | Files |
| `app.quit` | | Quit without asking about unsaved changes (the `app.quit` command, like ⌘Q, asks) |

Data merge runs through `engine.execute` like any command: `data.source.select`, `data.fields`, `data.placeholder.add` /
`.remove`, `data.options`, `data.preview` / `data.preview.stop`, `data.merge`. `data.merge` creates and activates a new
merged document; the template stays as it was ([agents.md](agents.md#data-merge)).

Headless window screenshots (locked screen, hidden window): `cargo run -p designcraft-ui-egui --example ui_shot -- script.jsonl`, where each line is one of the requests above, `{"shot": "/abs/out.png"}` or `{"steps": n}` (renders the whole UI offscreen with wgpu).

The MCP server (`designcraft-cli mcp`) wraps the same methods for Claude and other agents.

### Click and drag limits

`ui.click` and `ui.drag` validate the whole gesture before queuing any input. They permit at
most 10,003 pending synthetic events, including control input already waiting for native frames. A
click uses `1 + 2 * count` events; a drag uses `1 + steps + 2 * count`. The count includes the
initial click or drag; further clicks occur at its endpoint. An empty queue therefore accepts
a single drag with up to 10,000 steps. `count` is a JSON integer from 1 through 255 and `steps` a
positive JSON integer when supplied; invalid or over-budget requests return `ok: false` and leave pending
input unchanged. Oversized gestures are rejected rather than truncated. Wait for queued input
to finish before retrying a request that would exceed the pending-event limit.

## Shared panel docking

Panel arrangements support tab grouping and reordering, horizontal and vertical splits,
and movable, resizable floating windows. A floating window holds tab groups split side by side
or one above another, like the dock area. Drag a panel tab, or use its context menu. On the
desktop each floating window is a window of its own; on the web floating windows stay within
the application viewport.

The window is one panel layout: the protected `canvas` panel (the document area) beside a
dock at the logical end edge, the right in left-to-right languages and the left in
right-to-left ones. Each built-in workspace has a default layout. Essentials and most others
have an outer column with the Properties · Pages · Layers group and an iconic column with the
other panels; Essentials Classic and Typography have their named iconic groups only. A panel a
workspace opens (Printing and Proofing: Swatches) is a group below Properties · Pages · Layers.
The `canvas` panel never moves, floats, closes or takes tabs; a panel or tab group dropped
over it floats there. Workspace saves retain its groups, active tabs, split sizes, floating
geometry, and the positions of hidden panels. Resetting a built-in workspace restores its
default dock. Preferences saved before the canvas joined the layout load with nothing lost:
the classic dock becomes the workspace's default layout, and a customized dock-only layout
becomes the dock's expanded columns (iconic if the dock was collapsed), with the remaining
panels (or the workspace's named groups) in the iconic column and its floating windows and
hidden-panel positions kept. A `ui.json` the app cannot read in full is copied to
`ui.unreadable.json` before it is saved again.

The `window.panel.*`, `window.workspace.*` and `ui.dock` commands are craft-ui's shared
docking commands (`DockCommand`), with the same ids and params in every craft app. `panel`,
`anchor`, `target` and `before` take a panel ID or its English name in any case; `onto` is
read as `anchor`.

- `window.panel.move {panel, anchor, zone?, before?, group?, placement?}` moves a panel into
  the target group. `zone` is `tab` (default), `center`, `left`, `right`, `top`, `bottom`,
  `above` or `below`; `before` reorders tabs by panel ID (`null` appends). Beside a panel in a
  dock column, `top` and `bottom` add a group above or below its group and `left` and `right`
  add a column on that side; in a floating window, `top` and `bottom` add a group and `left`
  and `right` are errors (craft-ui's `Placement::for_zone`). `group: true` moves the panel's
  whole tab group; `placement` is a serialized craft-ui `Placement` and takes precedence over
  `zone` and `before`.
- `window.panel.float {panel, x?, y?, width?, height?, group?, column?}` (and
  `window.floatPanel`) detaches a panel, its tab group (`group`) or its dock column
  (`column`). Without a position it floats 24 points inside the dock area's top-left corner,
  clear of the menu bar; a panel floats at its own size and a tab group or column at the size
  a drag would float it at. Given values replace these. A panel already alone in a floating
  window moves or resizes, and a tab group that is already a whole floating window keeps its
  rect unless the params change it.
- `window.panel.dock {panel, anchor?, zone?, before?, group?, column?, side?}` (and
  `window.dockPanel`) docks beside `anchor`, or in a new column of the dock at `side`
  (`Start` or `End`; a new dock when there is none). A panel that already leads that dock's
  last column stays where it is. `zone` places the panel beside `anchor`, so a `zone` without
  `anchor` or `side` is an error. Without `anchor` and `side`, a panel that floated from
  the dock goes back to that place, and another floating panel joins the first dock group. For
  the float-only Tabs panel (`tabs`), which has its own window outside the layout, float opens
  that window and dock closes it.
- `window.panel.activate {panel}` (and `window.panel {panel}`) reveals and selects a panel,
  restoring its saved position when possible; a panel in an iconic column opens as a flyout
  beside its icon. `window.panel.close {panel}` hides it.
- `window.panel.closeGroup {panel}` hides the panel and the panels tabbed with it, and
  `window.panel.closeOthers {panel}` hides the panels tabbed with it; the result lists them in
  `closed`.
- `window.panel.minimize {panel, minimized?}` minimizes the panel's group to its tab strip and
  `window.panel.iconic {panel, iconic?}` collapses its dock column to icons; without the flag
  they toggle.
- `window.workspace.new {name}`, `window.workspace.delete {name}` and `window.workspace.reset`
  (also `ui.dock.reset`) are `window.newWorkspace`, `window.deleteWorkspace` and
  `window.resetWorkspace`. `window.workspace {name}` switches workspaces as before.
- `ui.dock {action}` or `ui.dock {operation, …}` is the shape LightCraft, PDFCraft, CADCraft
  and SoundCraft use: `open`, `close`, `activate`, `float {rect}`, `moveFloating {rect}`,
  `move {target, zone?, before?}`, `setStackOpen {open}`, `resizeStack {height}` and
  `resizeSplit {path, size}`, with `rect` as `[x, y, width, height]`.
- `window.panel.window` (a panel into or out of its own window) and `window.workspace.lock` are
  not available in DesignCraft and return an error.
- `window.collapseToIcons` collapses every dock column to icons.

Panel IDs are stable across themes. Invalid IDs, non-finite or invalid geometry, and
invalid destinations return an error without changing the arrangement.

`window.panel.layout {action}` also accepts the shared, externally tagged action schema.
For example, `{"action":{"MoveFloating":{"panel":"properties","rect":[44,66,360,450]}}}`
updates a floating window's position and size; `{"action":{"ResizeSplit":{"path":[],"size":{"Ratio":0.35}}}}`
resizes the root divider (`false`/`true` path entries select first/second children), and
`{"action":{"ResizeWindowSplit":{"panel":"swatches","path":[],"size":{"Ratio":0.6}}}}` resizes
a divider in the floating window that holds `panel`, by its path from the window's root.
Floating windows saved as a stack of groups (`{"groups": […], "rect": […]}`) load as groups
one above another, sized by their saved heights.
`Move` with `placement: {"Tab":{"before":"swatches"}}` reorders tabs, and
`SetStackOpen {panel,open}` / `ResizeStack {panel,height}` control accordion entries.
The UI dispatches these same commands. Unknown panel IDs, invalid geometry, and stale
split paths return an error without changing the workspace.

`app.tablePanel` (including Shift+F9) toggles the Table panel. `ui.set` with `closePanel`
closes the last explicitly opened panel; `dockExpanded:false` collapses every dock column
to icons and `dockExpanded:true` expands the outer column. Activating a floating panel also
raises its group. These operations change only UI layout.

Clicking an icon in an iconic column opens its group as a flyout beside the column, on the
canvas side; clicking the icon again closes it. With Auto-Collapse Iconic Panels on (the
default; a column bar's menu toggles it), Escape or a click outside the flyout closes it too.

`ui.docking` returns the panel layout in use; `ui.inspect` includes the same object as
`docking`:

```json
{"layout": {"root": {"Split": {"axis": "Horizontal", "size": {"FixedSecond": 297.0},
   "first": {"Tabs": {"panels": ["canvas"], "active": 0}},
   "second": {"Dock": {"side": "End", "columns": [
     {"groups": [{"panels": ["properties", "pages", "layers"], "active": 0}], "width": 260.0,
      "iconic": false, "iconic_width": 37.0},
     {"groups": [{"panels": ["stroke"], "active": 0}, …], "width": 260.0, "iconic": true,
      "iconic_width": 37.0}]}}}},
  "floating": []},
 "customized": false, "focused": "canvas", "flyout": null, "autoCollapse": true,
 "rightToLeft": false, "hidden": [],
 "hitRects": [{"hit": {"Tab": "properties"}, "rect": [983.0, 0.0, 74.0, 27.0]},
              {"hit": {"DragBar": "properties"}, "rect": [1190.0, 0.0, 56.0, 27.0]},
              {"hit": {"Icon": "stroke"}, "rect": [1243.0, 14.0, 37.0, 30.0]}, …]}
```

`layout` is craft-ui's serialized `Layout`; `customized` is false while the workspace's
default layout is in use; `focused` is the panel (or `canvas`) last clicked or selected, or
null; `flyout` is the panel whose iconic-column flyout is open (as drawn in the last frame,
or as a command that opens one sets it); `autoCollapse` is the
Auto-Collapse Iconic Panels setting; `hidden` lists closed panels that remember their place.
`hitRects` lists where each docking gesture starts in the last frame, from craft-ui's
`DockArea::hit_rects`: `hit` is a serialized craft-ui `Hit` (`Tab`, `DragBar`, `Border`,
`Overflow`, `ColumnBar`, `Collapse`, `StackToggle`, `Title`, `Close`, `Icon`, `IconGrip`,
`Resize` with its edge, or `Divider`), naming groups, columns and windows by their first panel;
`rect` is `[x, y, width, height]` in points. Drag from a rect's centre with `ui.drag`, or click
it with `ui.click`, to script a gesture.
