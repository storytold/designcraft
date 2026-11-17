# Plug-ins

DesignCraft runs **command plug-ins**: WebAssembly modules that act on the document through the same
commands as the menus, the CLI, the control channel and MCP. The sandbox, the manifest
(`craft-plugin.json`), the `.craftplugin` bundle, catalogs and installing from GitHub come from the
shared [`craft-plugin`](https://github.com/storytold/craft-libs/tree/main/crates/plugin) crate
([RFC-0041](https://github.com/storytold/craft-contrib/blob/main/rfcs/0041-shared-plugin-runtime.md));
its README describes the manifest and the ABI for plug-in authors. A plug-in for DesignCraft names
`designcraft` in its manifest's `apps`:

```json
"apps": { "designcraft": { "api": 6 } }
```

## Using plug-ins

- **Plug-ins ▸ Manage Plug-ins…** lists what is installed and installs more: from a `.craftplugin`
  file, from a GitHub repository (`owner/repo` or `owner/repo@tag`: the release's `.craftplugin`
  asset, checked against its `SHA256SUMS`) or from a catalog (an `index.json` on GitHub, or a local
  `file://` one) you add there. **Check for Updates** asks each plug-in's catalog or repository.
- Every install opens a confirmation first: the plug-in's access, the commands it may call, its kind,
  where it comes from and its SHA-256, and a plain warning when a repository is in no catalog you
  use. Nothing is written before **Install**.
- Each installed plug-in's commands are in the **Plug-ins** menu. A command that declares parameters
  asks for them in a dialog generated from its manifest.
- Plug-ins are kept in `Plug-ins/` in DesignCraft's settings folder (beside `prefs.json`). The CLI and
  the MCP server install for their session only. A plug-in's settings are kept in `prefs.json`
  (at most 64 KB per plug-in, keys up to 128 characters).

## Commands

| Command | Parameters |
|---|---|
| `plugin.list` | `{}` → the plug-ins, folders that couldn't be read, the catalogs, the last run |
| `plugin.install` | `{path}`, `{data: base64, name?}`, `{repo: "owner/repo[@tag]"}` or `{catalog, id, version?}`; `review: true` checks and describes the bundle without installing; `sha256` installs only that bundle |
| `plugin.update` | `{id?, apply?}`: the plug-ins with a newer release and the `plugin.install` parameters that get it; `apply` installs the ones that ask for no other permissions |
| `plugin.remove` | `{id}` (its settings go too) |
| `plugin.run` | `{id, command?, params?, fuel?}` |
| `plugin.catalog.list` | `{url?}`: the packages of one catalog, or of every catalog added |
| `plugin.catalog.add` | `{url, remove?}` |

```sh
designcraft-cli run --sample --cmd 'plugin.install={"path":"tidy.craftplugin"}' --cmd 'plugin.run={"id":"io.github.example.tidy"}' --export out.pdf
```

Without network access (the CLI, MCP) only files and `file://` catalogs install.

## What a plug-in may do

- A run is **one undo step**, named after the plug-in. When it fails (a trap, an exhausted
  instruction budget, a non-zero status) the document, selection and undo history are put back.
- `permissions.access: "read"` plug-ins may call any allowed command, but a call that changes the
  document is undone at once and answered with an error. `permissions.commands` narrows the commands
  further (`"select.*"`, `"swatch.create"`).
- A call that would open a dialog or a file picker is put back and refused.
- Never allowed, whatever the manifest says: plug-ins themselves (`plugin.*`), Undo and Redo, files
  (`file.*`, and any parameter named `path`, `paths` or `folder`), the app and its windows (`app.*`,
  `window.*`), preferences (`prefs.*`, `pref.*`), the tool in hand (`tool.*`), colour settings and
  profiles, books, libraries, links and linked text, data merge, reloading fonts, the clipboard and
  the Content Collector, other open documents and the undo history (`document.list`,
  `document.history`), and commands that run other commands (`script.*`, `edit.preview` and its
  commit and cancel).

Not yet: plug-in panels (`ui_show` runs are reported, not shown), `permissions.saveFile`, and the
Plug-ins menu in the macOS menu bar (Manage Plug-ins is in Quick Apply there).
