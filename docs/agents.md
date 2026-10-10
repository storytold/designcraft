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

`run` works through its arguments in order. `--page`, `--scale` and `--pdf-options` apply to the `--export` /
`--all-pages` that follow them, so one run can write several sizes
(`run --sample --scale 0.5 --export small.png --scale 2 --export large.png`); one with no export after it is an error.

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
