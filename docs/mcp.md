# MCP server

`designcraft-cli mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server that lets Claude and
other agents drive DesignCraft: create documents, lay out frames and text, apply styles, place images, and look at
the result as PNG images. It speaks newline-delimited JSON-RPC 2.0 on stdin/stdout (protocol `2025-06-18`, also
`2025-03-26` and `2024-11-05`); logs go to stderr. The implementation is the `designcraft-mcp` crate.

## Register it

### From source (development)

With Rust and Cargo installed, run this from the repository root:

```sh
cargo install --path apps/designcraft-cli --locked
```

Cargo copies the executable into its installation root's `bin` directory, normally
`~/.cargo/bin` (Windows `%USERPROFILE%\.cargo\bin`). If you set `CARGO_INSTALL_ROOT`,
Cargo's `install.root` configuration, or `CARGO_HOME`, use that installation's `bin`
directory instead. Unlike a symlink into `target/release`, this copy survives
`cargo clean`. Repeat the install command after source changes to update it.

Ensure that directory is on `PATH`. For a default install in a POSIX shell:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
designcraft-cli --help
```

Persist the PATH setting in your shell configuration (on Windows, add the directory
to your user Path). The **MCP client process** must inherit that PATH too; restart
the client after changing it. A GUI-launched client may not read shell configuration.
If it still reports `ENOENT` or "executable not found", use the absolute path to the
installed binary in its registration instead of the bare name.

### Headless and connected registration

After installation, register the CLI by name:

```sh
# Headless: an in-process engine, no window
claude mcp add designcraft -- designcraft-cli mcp

# Drive the running desktop app (install/start the desktop app separately)
designcraft --control 7979
claude mcp add designcraft-app -- designcraft-cli mcp --connect 7979
```

Other MCP clients use the same command line, e.g. in a JSON config:

```json
{
  "mcpServers": {
    "designcraft": {
      "command": "designcraft-cli",
      "args": ["mcp"]
    }
  }
}
```

If the client cannot find the CLI on PATH, substitute the absolute installed path
(not a path under `target/`), for example:

```sh
claude mcp add designcraft -- /absolute/install/root/bin/designcraft-cli mcp
```

### From an installed release

The release packages ship `designcraft-cli` alongside the desktop app, so no build is needed:

| Install | CLI |
|---|---|
| Windows (MSI) | `C:\Program Files\DesignCraft\designcraft-cli.exe` by default (wherever you installed it otherwise), not on `PATH` |
| Linux (deb, rpm) | `/usr/bin/designcraft-cli` |
| macOS | the separate `designcraft-cli-<version>-macos-<arch>.zip` release asset (the `.app` holds only the desktop app) |

```sh
# Windows, default install folder
claude mcp add designcraft -- "C:\Program Files\DesignCraft\designcraft-cli.exe" mcp
# Linux, or macOS with the CLI unzipped onto PATH
claude mcp add designcraft -- designcraft-cli mcp
```

## Confining file access

```sh
designcraft-cli mcp --automation-read-root /work/project --automation-write-root /work/project/out
designcraft-cli mcp --automation-read-root /work/project --automation-read-root /work/shared-assets \
  --automation-write-root /work/project/out --automation-write-root /tmp/scratch
```

```json
{"mcpServers": {"designcraft": {"command": "designcraft-cli",
  "args": ["mcp", "--automation-read-root", "/work/project", "--automation-write-root", "/work/project/out"]}}}
```

The two flags (VectorCraft, PhotoCraft and the other Craft apps take the same ones, so one client
config works across the suite) keep the agent to the folders you give it:

- Every file a command reads must lie inside a **read root**: `open_document`, `file.open`,
  `file.openIdml`, `place_image` and `file.place` / `place.load` / `media.options` with a `path`,
  `links.relink`, `links.relinkFolder`, `color.loadProfile`, `swatch.load`, `library.open`,
  `book.open`, `data.source.select`, `data.merge` and `file.importXml` / `xml.loadDtd` with a `path`,
  and `file.recovery.list` / `file.recovery.open` with a `dir`. A DesignCraft document names more
  files than the agent does: linked images, data sources, book chapters, the `Document Fonts` folder
  beside a document, an IDML package's linked images and the files an image cell of a merge names.
  Those are read through the same checks, so a path in a document that leads outside the read roots
  reads as a file that isn't there.
- Every file a command writes must lie inside a **write root**: `save_document`, `export_png`,
  `render_page` with a `path`, `file.save`, `file.saveAs`, `file.saveACopy`, `file.exportPdf`,
  `file.exportEpub`, `file.exportFixedEpub`, `file.exportHtml`, `file.exportText`, `file.exportXml`,
  `file.exportIdml`, `file.printBooklet`, `snippet.export`, `swatch.save`, `library.new` and the
  library's saves, `book.new`, `book.add`, `book.paginate`, `book.syncStyles`, `book.exportPdf`,
  `file.package`, `links.copyTo` and `file.recovery.save` / `file.recovery.open` with a `dir`
  (reopening removes the entries it read). `file.package` and `links.copyTo` check their folder
  before they create anything.
- Each flag may be given more than once (#1552): a path inside any of the read roots may be read,
  one inside any of the write roots written, so one session can read a project folder and a shared
  assets folder, or write to an output folder and a scratch folder, without widening a root to
  their common parent (which the roots don't grant). A folder given twice counts once.
- Read and write access are separate. Give the same folder to both flags for both. A kind left out
  grants none of its access: with only `--automation-read-root` nothing can be written to disk.
  Without either flag nothing changes.
- A path is made absolute against the server's working directory, then its links are followed: a
  file (or folder) link that leads out of the roots is outside them. For a file that doesn't exist yet,
  its nearest existing folder's links are followed and the rest must be plain names, so `..` below a
  folder that doesn't exist is refused, as is a link that leads nowhere (writing through it would
  create its target). The result is compared with each root by whole folder names: `/work2/x` is not
  inside `/work`. On Windows case is ignored, UNC paths (`\\server\share\...`) are resolved like
  drive-letter ones, and device names (`CON`, `NUL`, `COM1`...) are refused.
- A refused path is an ordinary tool error the agent can read and correct, for example
  `automation path rejected: outside the write root /work/project/out: /etc/x.designcraft` (with
  several, `outside the write roots /work/project/out, /tmp/scratch: ...`) or
  `automation filesystem access is not granted: write authority is absent: /work/project/out/x.designcraft`.
- The roots must be existing folders; a missing one stops the server with an error. Each flag takes
  one folder per use (`--flag <dir>` or `--flag=<dir>`) and may be used again for more.

The flags confine the in-process server, so `--connect` together with them is an error: the server
would otherwise talk to an app that isn't confined. DesignCraft's desktop app has no equivalent flags
yet, so a running app (`designcraft --control`) is not confined. `--sample` is the operator's own
choice and is opened before the roots apply.

What the roots do not cover: commands that take no path still read and change the open document,
and `inspect_document` returns it. The system font scan, the installed fonts that `file.package`
copies, and the app's own recovery folder are fixed paths, not agent input, and are not checked. A
`Document Fonts` folder and each font file in it are checked before they are opened, so a link that
leads out of the read root is skipped with a note. `file.print` hands a PDF to the system's print
spooler, which is not a path the agent names.
Anything that reads or writes a path goes through one place in the engine (the `io` module). The
crates below the engine have no file access of their own: the IDML import and the font folders take
a reader or an access check from the engine, and ICC profiles arrive as bytes. A test scans the
source of every crate for the file system, however it is imported or spelled, and lists the few
files it excludes and why.

The checks hold for every thread of the server process. A path is checked, then opened: another
program that swaps a link in between can still win that race, so the roots keep an agent to the
files it was given rather than guarding against other software on the computer.

## Modes

| | Headless (`mcp`) | Connected (`mcp --connect PORT` or `HOST:PORT`) |
|---|---|---|
| Engine | In-process `designcraft_engine::Session`, starts with an empty Letter document (`--sample` opens the sample magazine) | The app's session, through its loopback control channel ([control-protocol.md](control-protocol.md)) |
| Rendering | `designcraft-render` (CPU) | The app's renderer |
| Window tools (`screenshot`, `click`, `drag`, `menu_list`, `ui_inspect`, `ui_set`, `dialog_*`) | Return an error explaining how to connect | Work |
| UI-only commands via `execute` (`view.*`, `window.*`, `help.*`, `edit.dynamicSpelling`, and `app.*` except `app.links`) | Error | Work |

In connected mode every tool maps to a control-channel method (`engine.execute`, `ui.render`, `ui.pointer`, …), so
what the agent does shows up live in the window. A failed request is not resent: the operation may have completed,
so inspect the document before retrying. The next request re-establishes the connection.

## Coordinates

Points (1/72 in), y down, in **spread** space: on a single-page spread the page's top-left is (0, 0); on a facing
spread the right page starts at x = page width. `inspect_document` lists page bounds per spread. Page indices in
`render_page` / `export_png` are 0-based. Commands that create items take `spread`: a body spread index, or
`{"kind": "parent", "index": 0}` for a parent spread (its index in `parents` of `inspect_document`); any other
value is an error. `pointer` events in connected mode may use `"space": "screen"` (egui points,
see `ui_inspect` → `canvasRect`).

## Text offsets

Positions in a story's text are **UTF-8 byte offsets**, not character counts: `anchor` / `focus` of `text.select`,
`start` / `end` of `story.replaceRange` and of `find.find` matches, the `pos` that `text.placeCaret`, `text.insert`
and `text.move` return, a story's `length` in `get_story` and `inspect_document`, and its `overset` position in
`get_story`. A character outside ASCII takes 2 to 4 bytes (é 2, — and は 3), so the story `はただ商店` has length 15
and its second character starts at 3. An offset inside a character snaps back to the start of that character, except
in `story.replaceRange`: there an offset inside a character, an `end` past the end of the story or a `start` after `end`
is an error that gives the story length in bytes, and the story is left unchanged. A `find.find` or `find.next` match
that has a `cell` (`{table, row, col}`) is in that table cell's text, and its `start` / `end` count from the start of
the cell.

## Text and image fitting

Hyphenation is on by default for new text (`hyphenate: true`). To turn it off, select the text or a text frame,
then call `execute`:

```text
execute          {"command": "type.para", "params": {"attrs": {"hyphenate": false}}}
```

With a text selection, this formats the selected paragraphs; with selected text frames, it formats their whole stories.

Use `execute` with `object.fit` to fit a selected graphic frame (or pass `ids`):

| `mode` | What it does |
|---|---|
| `fillProportionally` | Preserves aspect ratio, covers the frame and clips overflow. |
| `fitProportionally` | Preserves aspect ratio, shows the whole image and may leave empty space. |
| `fitContentToFrame` | Stretches the content to the frame's width and height. |
| `centerContent` | Keeps the current size and positions content at the fitting alignment (center by default). |
| `fitFrameToContent` | Resizes the frame to the content's bounds. |

## Tools

| Tool | What it does |
|---|---|
| `list_commands` | Every command (id, label, menu path, shortcut, params, enabled), with `filter` / `enabledOnly` |
| `execute` | Run any command: `{command, params}`. A params key the command doesn't take is a JSON-RPC `-32602` (invalid params) error naming it, as is an argument a tool doesn't take; `batch` checks every step before running the first |
| `batch` | Run `commands: [{command, params}]` (or `script` text, one `command.id {json}` per line) in order, stopping at the first error (reports `failedIndex`); `"$N.path"` parameters use earlier results ([agents.md](agents.md)) |
| `inspect_document` | Pages, spreads, items (ids, bounds, fill, story), stories (overset), layers, styles, swatches, selection |
| `get_story` | Text, frames, paragraphs, lines, overset of a story (`story` or text `frame`) |
| `set_story_text` | Replace a story's text (`\n` = new paragraph) |
| `new_document` | `file.new` (preset, intent, units, width, height, pages, startPage, facingPages, columns, gutter, margins, bleed, slug, title, primaryTextFrame) or `sample: true` |
| `open_document` / `save_document` | Native `.designcraft` files |
| `place_image` | Image from `path` or `base64` into `frame`, the selected frame, or a new frame at x, y, width |
| `render_page` | Render a page → MCP image content (PNG, base64); optional `path` to also save it |
| `export_png` | Write a page as PNG (or JPEG by extension) |
| `screenshot` | Whole app window → image content (connected only; captured to a temp file and read back) |
| `select_tool` | Activate a tool (`selection`, `type`, `rectangleFrame`, `line`, …) |
| `pointer` | Mouse gesture events (`down`/`drag`/`up`/`move`/`doubleclick`) for the active tool, optional `tool` |
| `key` | Key press with modifiers (headless: active tool → command shortcuts → tool shortcuts) |
| `type_text` | Type at the text insertion point (or into the focused dialog field in the app) |
| `ime` | Input method events at the Type tool's caret: `preedit` (marked text in place, `active` = converting clause in characters), `commit` (typed as one step) |
| `click` / `drag` | Real pointer input at screen points (connected only) |
| `menu_list`, `ui_inspect`, `ui_set` | Menu tree, UI state, change UI state (connected only) |
| `dialog_set` / `dialog_confirm` / `dialog_cancel` | Fill in and close the open dialog (connected only) |

Tool failures come back as results with `isError: true` and a message the model can act on; protocol errors use
JSON-RPC error codes. Resources: `designcraft://document` (document.inspect) and `designcraft://commands`.

## Example

```text
new_document     {"preset": "A4", "margins": 36}
execute          {"command": "frame.create", "params": {"rect": [36, 36, 559, 200], "content": "text", "text": "Hello"}}
                 → {"id": 8, "story": 9}
set_story_text   {"story": 9, "text": "Summer Fair\nSaturday in the park"}
execute          {"command": "type.char", "params": {"size": 28}}
place_image      {"path": "/tmp/photo.jpg", "x": 36, "y": 220, "width": 523}
render_page      {"page": 0}       → PNG image
```

Or with the Type tool, the way a user would:

```text
pointer    {"tool": "type", "events": [{"kind": "down", "x": 72, "y": 72}, {"kind": "drag", "x": 300, "y": 200}, {"kind": "up", "x": 300, "y": 200}]}
type_text  {"text": "Hello"}
key        {"key": "Enter"}
key        {"key": "Escape"}
```

## Community links

`app.links` returns the ArtCraft Discord (https://discord.gg/artcraft), the website (https://getartcraft.com), DesignCraft's app page (https://getartcraft.com/apps/designcraft), its GitHub repository (https://github.com/storytold/designcraft) and the issue tracker. In the running app, the UI commands `help.discord`, `help.appPage`, `help.github`, `help.issues`, `help.website`, `help.app {app}` open them in the browser and `help.about {tab?}` shows the About window (tabs `about`, `contributors`, `models`). The server's `serverInfo.websiteUrl` is the app page.
