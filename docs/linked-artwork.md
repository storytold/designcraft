# Linked artwork in native working files

By default, native `.designcraft` documents retain an embedded copy of placed artwork, even
when the Links panel records an external source. Existing documents keep this behavior.

To save a smaller working document, open **Window › Links** and select **Keep linked artwork
external on save**, then save. The choice belongs to the document and can be undone. Placed
files that have source paths are then stored as references, without their full originals in
the native ZIP. Artwork without a source path, including an item explicitly set to **Embed**,
stays embedded. Placed audio and video also stay embedded. **Relink** attaches an embedded item to a source file again.

Scripts can choose the same mode:

```text
links.storage {"mode":"linked"}
file.saveAs {"path":"/project/layout.designcraft"}
```

Or set it while saving:

```text
file.saveAs {"path":"/project/layout.designcraft","embedImages":false}
```

The Links panel warns before changing the storage option: linked-only files depend on the external originals. Keep sources with the document, use Package to collect them, or Embed for a self-contained file.

The next `file.save` remembers that choice. `embedImages:true` switches back to including
linked originals; `links.storage {"mode":"embedded"}` does the same. `file.saveACopy` accepts
the option without changing the open document's mode, path or saved state.

Working saves retain the first-page picture used by Recent Files. Save and Save a Copy add the
`.designcraft` extension when it is missing, including when linked storage is selected.

Source paths are stored relative to the working document where possible, so moving the layout
and its `Links/` folder together keeps them connected. Opening a native file loads its linked
originals into memory for editing and full-resolution export. Changes to a source during the
session still appear as Modified in the Links panel and can be accepted with Update.
Reopening a changed linked PDF recalculates its saved crop box from the new source bytes.
Linked-only open and save use the same path checks as other document links: paths to other
computers (apart from the document's own network share) and URLs are refused before file-system
access. Device paths are rejected or skipped as nonregular files before reading originals.
The Links panel explains refused paths. Relative references resolve from
the source document, including when switching to linked storage with Save As or Save a Copy.

If an original cannot be found when opening, the layout opens with a warning and the missing
link can be relinked. There is no saved preview for unresolved artwork in this first version.
If the document still has the original cached in memory but the source has disappeared, a
linked-only save fails instead of discarding that cached copy: use Relink or Embed first.
An unresolved link cannot be embedded until its original is supplied.

Package collects available originals in `Links/`; its native document retains the chosen
storage mode. PDF and print output use the loaded full-resolution artwork. This option reduces
working-file storage and serialization. Encoded originals remain resident for reliable export,
undo and recovery. External artwork reads are limited to 512 MiB
per source file; use embedded storage for larger sources. Automatic opening also limits the
combined original buffers to 4 GiB (counting existing shared buffers once) and attempts at most
4,096 sources. Repeated placements of a loaded source share its buffer. Over-budget links stay
unresolved with warnings; use Relink or Update for individual sources, or embedded storage.
Warning details are capped at 32 plus an omitted-count summary; the Links panel shows all links.
These limits cover encoded originals, not decoded rendering memory. EPS retains the existing preview
rendering behavior; this change does not add a PostScript interpreter.

Recovery, clipboard/snippets and `file.serialize` remain self-contained. Byte-only opening
(`file.openBytes`, including the web app) has no project folder from which to resolve external
files, so it keeps any embedded data and warns about unresolved artwork instead of reading
paths supplied inside a document. The Links panel option is disabled on the web.
Linked-only saves of a byte-opened layout with unresolved relative references are refused:
use Relink to supply their original paths first. A save destination is never their source folder.
Package resolves missing relative references against the source layout before moving the copy,
so an unrelated file in the package folder cannot replace missing artwork.

Working files that omit linked originals use native format 4. Portable or complete working files
keep the older embedded format even when linked mode is selected. Older applications must ask for an update
for a format-4 file rather than open it with silently missing artwork. Ordinary embedded files remain
format 2 (format 3 when retaining EPS originals); opening older files never converts them to external storage automatically.

Native format 1 layouts retain their original first-baseline calculations when opened. Saves
containing this compatibility setting use format 5, including embedded saves, so older readers
refuse them instead of silently moving text. Current documents without this setting remain
format 2 (embedded), format 3 (embedded EPS) or format 4 (linked). Changing the first-baseline kind in Text Frame
Options adopts the current calculation for that frame. Reconfirming the same kind or changing
unrelated frame options retains compatibility.
