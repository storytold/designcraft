# Driving DesignCraft from agents and scripts

Every user-visible action in DesignCraft is a command (`crates/engine/src/cmd`), and every command is reachable
three ways — pick whichever fits the agent:

| Surface | Use it when | Entry point |
|---|---|---|
| **CLI** | One-shot jobs, shell pipelines, CI: build or edit a document and export it | `designcraft-cli run`, `script`, `app`, `describe`, `commands` |
| **MCP** | Claude and other MCP clients; images of pages and the window come back as content | `designcraft-cli mcp` ([mcp.md](mcp.md)) |
| **Control channel** | Your own client driving the running app (JSON lines over TCP) | `designcraft --control 7979` ([control-protocol.md](control-protocol.md)) |

## Discover commands

```sh
designcraft-cli commands footnote        # every command whose id/label/menu mentions "footnote"
designcraft-cli describe xref.insert     # label, menu path, shortcut and parameters of one command
```

Parameters are documented on each command (`params`), e.g. `{anchor? | paragraph?: text to find | story? + para?, format?}`.

## Scripts: several commands, later ones using earlier results

A script is one command per line (`command.id {json params}`; `#` comments), JSON lines, or a JSON array of
`{command, params}`, in UTF-8 with or without a byte order mark. A parameter string `"$N.path"` becomes that value
of step N's result: N is the 0-based step index (comments and blank lines are not steps, `$last` is the previous
step) and the path goes through result fields and array indices (`"$1.story"`, `"$3.ids.0"`); `"${N.path}"`
interpolates inside longer strings. Any other string starting with `$` stays as written (`"$1-$2"`, `"$5 off"`), and
`$$` writes a literal `$` anywhere (`"$$1"` passes `$1`, a GREP group). In text parameters `\n` starts a new paragraph and `\u2028` is a forced line
break (`\u000b`, Word's line break, is read as one too). Most commands ignore parameters they don't know instead of rejecting them, so check names with `describe`.
`designcraft-cli script --help` prints this reference.

```sh
cat > page.dcs <<'DCS'
frame.create {"rect": [72, 72, 540, 300], "content": "text", "text": "Results of the survey"}
text.select  {"story": "$0.story", "anchor": 0, "focus": 0}
footnote.insert {"text": "Survey run in 2026."}
frame.create {"rect": [72, 320, 540, 500], "content": "text", "text": "See ."}
text.select  {"story": "$3.story", "anchor": 4, "focus": 4}
xref.insert  {"paragraph": "Results", "format": "Paragraph Text & Page Number"}
DCS
designcraft-cli script page.dcs --save page.designcraft --export page.pdf --export page.png
designcraft-cli script page.dcs --connect 7979      # the same steps, live in the running app
```

The result is JSON: `{"completed": n, "results": [...]}`, plus `failedIndex` / `failedCommand` / `error` when a step
fails (the exit status is non-zero; `--keep-going` records errors and continues). `run --cmd ID=JSON` accepts the
same references, and MCP's `batch` tool takes `commands` or `script` text with them.

`run` works through its arguments in order. `--page`, `--scale` and `--pdf-options` apply to the `--export` /
`--all-pages` that follow them, so one run can write several sizes
(`run --sample --scale 0.5 --export small.png --scale 2 --export large.png`); one with no export after it is an error.

`--export` writes the file type its extension names: `.png` or `.jpg` (a page), `.pdf`, `.designcraft`, `.idml`,
`.epub`, `.html`, and `.rtf` or `.txt` (the story being edited or of the selected frame). A name with no extension
is a PNG of the page; another extension is an error, and nothing runs.

## Color themes

`color.extractTheme {id?}` previews up to five RGB colours from a placed raster image (or the selected image).
`swatch.addColorTheme {colors, name?}` adds that palette as a colour group in one undo step.
Extraction samples the whole source image on a bounded grid, weights partial transparency, and ignores fully transparent pixels.
The image must contain at most 16 million pixels. PDF/SVG graphics are unsupported. This is local median-cut extraction,
not Adobe's palette algorithm. In the UI, click an image with the Color Theme Tool (Shift+I), or choose Color Theme…
in the Swatches panel menu with one image selected. Cancel keeps the document unchanged.

## Data merge

`data.source.select {path, sheet?}` links a CSV/TSV/`.xlsx` file to the document (`data.source.update` re-reads it,
`data.source.remove` drops it); `data.fields` lists its fields. `data.placeholder.add {field, role?, story?, at?, end?, item?}`
marks text or a frame as a field (`role`: text, image, qr, hyperlink); `data.options` sets records, tiling and image
fitting; `data.preview {record}` / `data.preview.stop` show one record without saving it.

`data.merge` now **creates a new document** and leaves the template unchanged (the old `spread` parameter, which
appended pages to the template, is an error). It uses the linked source, or inline `csv` / `rows` / `path` / `bytes`:

```sh
echo 'data.merge {"path": "people.csv", "records": "range", "range": "1-20"}' | designcraft-cli script - --in template.designcraft --save merged.designcraft
```

## The running app

```sh
designcraft --sample --control 7979 &
designcraft-cli app type.changeCase '{"case": "title"}'            # any command
designcraft-cli app --method ui.screenshot '{"path": "/tmp/w.png"}' # any control-channel method
```

`ui.menu.invoke {command}` without params acts like choosing the menu item: a command labelled "…" opens its
dialog, which `ui.dialog.set` / `ui.dialog.confirm` fill in and close like a user would.

## Parent pages

In the Pages panel, right-click a parent row and choose **Delete Parent**. The equivalent
command is `layout.parents.delete {id}`, using the parent spread's numeric ID. It deletes that
parent's shared artwork and clears its assignments from document pages and other parents.
Local override copies remain as ordinary editable objects; document pages and their own content
remain. Undo restores the parent and its relationships in one step. Deleting the last parent
returns the canvas to document pages. Parent navigation is a separate operation.
