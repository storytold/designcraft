# Linked artwork

A graphic placed from a file is linked to it. The document stores the link, a small preview (proxy) and the file's content hash, not the file's bytes. A large catalogue opens fast and its file stays small.

## What links and what embeds

| Placed from | Stored as |
|---|---|
| A file on disk (`file.place {path}`): rasters, PDF, EPS, SVG, PSD, and the other file types Place accepts | Linked |
| `file.place {path, embed: true}` | Embedded |
| Bytes with no file: paste, drag of bytes, base64 placements | Embedded |
| Any placement on the web build | Embedded |
| IDML and INDD imports | Linked, using the graphics the file references; the stored preview is the proxy |

Video and audio link as well, but have no image proxy (the poster frame is used when there is one). An SVG that references other files resolves them relative to the SVG's own path.

Placing reads each file once and links the graphic to it at once, as InDesign does, with the file's size and modification time as its fingerprint. The bytes just read stay in memory for drawing, so nothing is read again. Placing returns without decoding or hashing the file, so placing many large images doesn't stop the app. A background thread then makes each graphic's content hash and proxy, in placement order. Until the hash is in, the link's status goes by size and time; a time from the last 2 seconds, or none, can't vouch for the bytes, and the hash decides once it is known. A save in that time makes the proxies still missing and writes the links without a hash. When such a document is opened, a file that matches by size and time is hashed in the background, so the next save has the hash.

Linked bytes are read when a render, export or check needs them. They are held in a shared cache of 512 MiB; a file larger than the cache is read for one use and not kept. A linked file larger than 1 GiB is not loaded.

## Proxies

A proxy is a copy of the image no larger than 1024 px on its long side: JPEG at quality 80 when the image is opaque, PNG when it has alpha, at most 512 KiB (the quality drops to fit). CMYK sources are converted through the colour management system. A placed PDF's proxy is a raster of the placed page.

The proxy is drawn while the original loads, and in place of the original when the link is Missing or Modified. The Links panel uses it for thumbnails. It is not a level of the image's display mip chain.

## Link status

| Status | Meaning |
|---|---|
| OK | A file at the link's path holds the recorded bytes. |
| Checking | Just after the document opens: the file is being looked for in the background, and hashed when it has the recorded size and another time. |
| Modified | A file exists, but its bytes differ from the recorded ones. |
| Missing | No file at either path, or a path that is not followed (a path on another computer, a device). |

Status comes from the content hash. A file with the recorded size and modification time is OK without a read. A file with the recorded size and another time is hashed on a background thread: a matching hash is OK and the new time is remembered, a different hash is Modified. Modification time alone is never trusted to decide Modified, because it is unreliable on network shares. A file whose file system reports no modification time is always told by its hash.

A Modified file's bytes are never adopted silently. The proxy stays on screen until you update or relink.

## Links panel commands

| Command | Does |
|---|---|
| `links.update` | Reloads modified links from their files (every link when none is named). A new hash and proxy are recorded. Undoable. |
| `links.relink` | Points every placement of a graphic at another file; a placed PDF keeps its page. Fails if the file is missing. |
| `links.relinkFolder` | Looks for missing links in a folder, or for the same name with another extension. Returns `relinked` and `notFound`. |
| `links.embed` | Copies the linked file into the document. Fails when the file is missing or modified: update or relink first. The link record is kept so Unembed can restore it. |
| `links.unembed {asset, dir?}` | Links an embedded graphic to its original file when that file holds the same bytes. Otherwise it writes the bytes into `dir` (reusing a file there with the same bytes) and links that. Not available on the web. |
| `links.goTo` | Selects the frames that show the graphic. |

`links.list` reports each asset's `storage` (`linked` or `embedded`) and `status` (`ok`, `checking`, `modified`, `missing`, `embedded`).

## Save format

A document with any linked asset is written as format version 5; otherwise the version stays 2, 3 or 4 as the content needs. Older readers reject version 5 as too new.

- A linked asset is saved as its record (absolute path, path relative to the document, size, modification time, SHA-256 hash) plus `proxies/<id>.jpg` or `.png`. The original is never written into the file.
- The relative path is recomputed for the save's destination, so a document and its Links folder can move together.
- Automatic recovery snapshots use the same layout. A save or snapshot makes the proxy of a linked graphic that has none yet; recovery snapshots make it on their background thread.
- **Save a Copy** with `embedLinks: true` copies every linked file in, making a portable document. A linked file that cannot be read stops that save with an error.

## Opening

Opening reads no original and asks nothing of the file system: a background check looks for each link at its absolute path, then at its path relative to the document. A match on size and time is OK without a read; other candidates of the recorded size are hashed. When both paths exist, the file whose hash matches wins.

## Old files with embedded images

A document saved before version 5 embeds every placed file. When such a document is opened from a path, each embedded asset that has a link whose file exists at the same size is compared byte for byte with that file in the background. Confirmed assets get a hash and proxy, and the next save writes them as links. After that save their bytes are released from memory. Assets whose file is missing or different stay embedded.

The status line reports once: "N images will be saved as links to their files (Links ▸ Embed keeps a copy)". `links.embed` undoes the migration for one image.

## Packages, export and preflight

- **Package** and **Copy Link(s) To** copy the original files disk to disk into `Links/` and relink to the copies, hash-checked. A linked asset stays linked, now to its copy. A missing or unreadable file is left as a missing link; no empty file is written in its place.
- **Export** (PDF, flattened output, EPS and the other formats, alpha wraps) uses the file on disk. When the link is Modified, the export uses the current file and warns. When the file is Missing, the export uses the proxy (a preview of at most 1024 px) and warns by name; with no proxy the image is skipped and the export warns ("image `x` was skipped: …"). A placed PDF whose layers are hidden or overridden is left out instead of drawn from the proxy, which shows every layer. HTML, EPUB, PowerPoint and IDML read linked files the same way; Package and Copy Link(s) To warn when they copy a Modified file.
- **Preflight** reports `missingLink` (error) for Missing links, with the reason when the path is refused, and `modifiedLink` for Modified ones ("exports use the file as it is now").

## Web builds

The web build cannot read files, so it embeds everything it places and cannot unembed. A document with linked assets opens with their proxies. Saving it on the web keeps the link records as version 5 and warns: "N linked files are saved as links only: the web version can't read them to embed them".
