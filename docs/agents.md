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
`{command, params}`. A parameter string `"$N.path"` becomes that value of step N's result (0-based, `$last` for the
previous step); `"${N.path}"` interpolates inside longer strings.

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

## Data merge

`data.source.select {path | bytes, name?, sheet?}` links a CSV, TSV, JSON, `.xlsx`, or `.xls` file
(`data.source.update` re-reads it, `data.source.remove` drops it). `.xlsm` is refused. `data.fields` lists the
columns plus the virtual fields `Source filename` and `Merge index`. `data.placeholder.add {field, role?, story?, at?, end?, item?}`
marks text or a frame (`role`: text, image, qr, hyperlink). An image placeholder can sit inside a story.
`object.qrCode {field?, source?, rect?}` can bind a QR frame to any column, including one that is not marked `#`.

`data.options` sets records, tiling, image fitting, and `skipWarnings` (off unless set). `data.source.affix` stores
prefix and postfix rules. `data.join` names a driving source and key links; with no driving source, records are
concatenated. `data.sort` sorts the combined list after that join or concatenation. `data.preview {record}` and
`data.preview.stop` show one record without saving it. Excel formulas and date formats are applied for data merge.
Placing a workbook as a table still uses cached `.xlsx` values and does not place `.xls`.

`data.merge` creates a new document and leaves the template unchanged (the old `spread` parameter, which appended
pages to the template, is an error). It uses the linked source, or inline `csv`, `rows`, `json`, `path`, or `bytes`.
`pdf` writes that merged document with the existing PDF export in the same run. Omitting `pdf` writes no file.
The Data Merge Grid tool drags a rectangle and calls `data.grid.create` with that rectangle and the spread. The
Create Grid button still calls that command for the page margin rectangle.

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
