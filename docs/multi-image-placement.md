# Multiple images and contact sheets

Choose **File > Place** (Cmd+D on macOS) and select several images. They load into a placement cursor in the order supplied by the file picker. The cursor shows the next image's thumbnail and, beside it, how many are loaded ("(6)").

- Click to place the next image at its natural size.
- Drag to draw its frame: it keeps the image's proportions, and shows the scale beside the pointer.
- Click an existing graphic frame to replace its contents.
- While drawing, Right/Left add/remove columns and Up/Down add/remove rows. Release to place a grid in row order with 6-point gutters. Extra images remain in the cursor; unused grid cells create no empty frames. A drag too small for the grid's cells shows and places one frame instead, as an ordinary drag does.
- Press Escape or select another tool to cancel the remaining placement.
- Switching to, opening or closing a document also cancels it, and goes back to the Selection tool.

For larger selections, choose **File > Place Contact Sheet…** after loading the images. Set columns, rows, gutter and page margin. All remaining images are fitted proportionally and centered in equal frames across as many new pages as needed. The default is 3 columns by 4 rows. New pages use the last document page's size and have no parent-page artwork. The entire contact sheet is one undo step.

A single selected file retains the normal Place behavior, including filling a selected frame and PDF import options. Several files go to the place cursor whatever is selected, even with a PDF first. A multi-page PDF in a batch pauses the batch at its import options; once confirmed, it queues after the images loaded before it and the rest of the batch follows. Cancelling them ends the batch, and the images already loaded stay. The place cursor holds up to 1000 images, counting those already in it when a batch adds to them. Rows and columns each range from 1 to 100. A batch containing an unreadable image fails without replacing the existing queue or changing the document. An invalid grid or contact-sheet area also leaves the document and queue unchanged.

Images here are PNG, JPEG, GIF, WebP, BMP, TIFF, SVG, PSD and EPS files.

## Automation

All placement behavior is available through the engine, CLI, control channel and MCP:

```json
{"command":"place.load","params":{"files":[{"path":"/images/one.png"},{"path":"/images/two.jpg"}]}}
{"command":"place.load","params":{"files":[{"path":"/images/three.png"}],"append":true}}
{"command":"place.grid","params":{"rect":[36,36,576,756],"columns":3,"rows":4,"gutter":6}}
{"command":"place.contactSheet","params":{"columns":3,"rows":4,"gutter":6,"margin":36}}
{"command":"place.cancel","params":{}}
```

Each source can instead supply `base64` and `name`, including on the web. `append` queues the images after those already loaded instead of replacing them. `place.grid` consumes only the images that fit; `place.contactSheet` consumes the entire queue. Undo removes placed frames/pages together, while the cursor remains at its current position in the queue. The UI command `app.placeDialog` takes `paths` for a File > Place of several files without the dialog.
