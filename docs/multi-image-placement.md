# Multiple images and contact sheets

Choose **File > Place** (Cmd+D on macOS) and select several images, or drop a selection of images from your file manager onto the document. Multiple images load into a placement cursor in the order supplied by the file picker. The cursor shows how many remain.

- Click to place the next image at its natural size.
- Drag to draw its frame.
- Click an existing graphic frame to replace its contents.
- While drawing, Right/Left add/remove columns and Up/Down add/remove rows. Release to place a grid in row order with 6-point gutters. Extra images remain in the cursor; unused grid cells create no empty frames.
- Press Escape or select another tool to cancel the remaining placement.

For larger selections, choose **File > Place Contact Sheet…** after loading the images. Set columns, rows, gutter and page margin. All remaining images are fitted proportionally and centered in equal frames across as many new pages as needed. The default is 3 columns by 4 rows. New pages use the last document page's size and have no parent-page artwork. The entire contact sheet is one undo step.

A single selected file retains the normal Place behavior, including filling a selected frame and PDF import options. Image batches support up to 1000 images. Rows and columns each range from 1 to 100. A batch containing an unreadable image fails without replacing the existing queue or changing the document. An invalid grid or contact-sheet area also leaves the document and queue unchanged.

## Automation

All placement behavior is available through the engine, CLI, control channel and MCP:

```json
{"command":"place.load","params":{"files":[{"path":"/images/one.png"},{"path":"/images/two.jpg"}]}}
{"command":"place.grid","params":{"rect":[36,36,576,756],"columns":3,"rows":4,"gutter":6}}
{"command":"place.contactSheet","params":{"columns":3,"rows":4,"gutter":6,"margin":36}}
{"command":"place.cancel","params":{}}
```

Each source can instead supply `base64` and `name`, including on the web. `place.grid` consumes only the images that fit; `place.contactSheet` consumes the entire queue. Undo removes placed frames/pages together, while the cursor remains at its current position in the queue.
