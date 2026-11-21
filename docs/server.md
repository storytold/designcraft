# Render server

`designcraft-server` fills a template's tagged frames from XML (text and images) and returns the
result as PDF, PNG, JPEG, `.designcraft` or IDML, over HTTP. Each request runs in its own engine
session and temporary folder, so requests don't share state.

```sh
cargo run --release -p designcraft-server -- --templates ./templates --listen 127.0.0.1:8080
```

| Option | Default | Meaning |
|---|---|---|
| `--listen ADDR` | `127.0.0.1:8080` | Address and port. Use `0.0.0.0:PORT` to accept other machines. |
| `--templates DIR` | none | `.designcraft` and `.idml` templates that requests name by path. |
| `--assets DIR` | none | A folder whose images the XML may name by absolute path. Repeatable. |
| `--workers N` | CPU count | Requests handled at the same time. |
| `--max-body MB` | 200 | Largest request body. |

The server has no authentication or TLS. Keep it on a trusted network, or put it behind a reverse
proxy that adds them.

## Preparing a template

Tag the frames the content goes into (Window ▸ Utilities ▸ Tags, or `xml.tag`). `xml.autoTag` tags
every untagged text and graphic frame at once: a frame whose Layers panel name is a valid XML name
gets that name, the others `text_1`, `image_1`, … in reading order. Elements fill the frames with
their tag in reading order, so a tag used on several frames takes repeated elements.

`GET /templates/{path}/structure` lists a template's tagged frames, which are the elements its XML
can fill.

## Endpoints

### `GET /health`

`{"ok": true, "version": "0.14.0"}`

### `GET /templates`

`{"templates": ["issue.designcraft", "flyers/a5.idml"]}`, paths relative to `--templates`.

### `GET /templates/{path}/structure`

`{"template", "root", "elements": [{"tag", "id", "kind", "text"?}], "tags": [{"name", "color"}]}`

### `POST /render`

A JSON body:

| Field | Meaning |
|---|---|
| `template` | A template's path inside `--templates`. |
| `templateBase64` | Or the template file itself, base64. |
| `templateType` | `designcraft` or `idml`. Default: from `template`'s extension, else `designcraft`. |
| `xml` | The content (optional). Elements fill frames with their tag; an element with `href` places an image. |
| `images` | `{"relative/path.jpg": "<base64>"}`: the files the XML names by relative `href`. |
| `autoTag` | `true`: run `xml.autoTag` on the template first. |
| `format` | `pdf` (default), `png`, `jpg`, `designcraft`, `idml`. |
| `page`, `scale` | For `png` and `jpg`: the page (from 1) and the scale (1 = 72 ppi). |
| `pdf` | `file.exportPdf` options, such as `{"preset": "press"}`. |
| `strict` | `true`: answer 422 when an element found no frame or an image couldn't be placed. |

```json
{
  "template": "issue.designcraft",
  "xml": "<Root><headline>Spring Issue</headline><photo href=\"img/cover.jpg\"/></Root>",
  "images": {"img/cover.jpg": "/9j/4AAQSkZJRg..."},
  "format": "pdf"
}
```

The answer is the file, with `Content-Type` set. The `X-DesignCraft-Report` header holds what the
import did, as JSON (non-ASCII characters escaped):

```json
{"import": {"placed": 2, "unmatched": 0, "failed": []}, "pdf": {"pages": 4, "warnings": []}}
```

`unmatched` counts elements without a frame for their tag; `failed` lists images that couldn't be
placed, with the reason. Without `strict` the file is still made.

Errors are JSON `{"error": "..."}`: 400 for a wrong request, 413 for a body over `--max-body`,
422 when the template or XML can't be used or `strict` failed (with `report`), 500 for a server
failure.

```sh
curl -s -X POST http://127.0.0.1:8080/render -H "Content-Type: application/json" -d @job.json -o out.pdf -D headers.txt
```

## What a request can read

While a request runs, the engine reads only its own temporary folder (the uploaded template and
images), `--templates` and the `--assets` folders, and writes only the temporary folder. An `href`
anywhere else is listed in `failed` instead of being read. Images a template links to outside those
folders keep the preview stored in the document.
