# Cloud storage

DesignCraft opens and saves layouts in cloud storage, and shares one, through `craft_ui::cloud`
and `craft_ui::share`. The pages, the account note, the Share split button and the conflict modal
are `craft-ui`'s. All twelve Craft apps therefore offer the same ones. What lives in this
repository is the half only DesignCraft can answer.

No concrete provider is connected in production. `designcraft` and `designcraft-web` implement
the `CloudApp` trait, which is what only an application can answer, and leave the backend slot
`CloudApp::cloud` reads from empty: nothing calls `CloudFiles::connect` or `ShareDialog::connect`
in either app's `main.rs`. A build today therefore shows no cloud menu entry, no account note and
no Share split button. `crates/ui-egui/tests/cloud_files.rs` connects an in-process mock backend,
defined in the test file itself, to exercise both the connected and the disconnected state.

## What a user would get, once a backend is connected

- **A Cloud section on the start screen.** Your own layouts sit on one side. What other people
  shared with you sits on the other. Folders can be walked into. Each document carries an overflow
  menu holding Open, Share and Work together. File › Open from the Cloud… shows the same page over
  an open layout.
- **Save As** offers This Computer and your cloud account side by side, then a folder and a name.
- **An account note** in the application bar. The badge is the account's initial.
- **A Share split button.** The label opens the share dialog. The chevron offers Work together,
  then Save a Copy, Export as PDF, Export IDML and Export EPUB. Both the note and the button draw
  only once the service says the capability behind them exists; the chevron's items are always on,
  since DesignCraft answers those itself.
- **A presence row** beside the account, showing who else is in the open layout. Nothing is drawn
  while a person works alone.

## What a layout is, here

The extensions are `.designcraft` and `.idml`, the same two the desktop file picker offers. The
content types are `application/vnd.designcraft+zip` and
`application/vnd.adobe.indesign-idml-package`. Each is matched against the start of what the
provider said. Neither the extension nor the content type is reliable on its own. A typed name with
no known extension becomes `.designcraft`. Bytes become an open layout through `file.openBytes`.
They go back through `designcraft_format::save`.

## Saving over somebody else

Every save carries the etag the layout was read at as its condition. A stored copy that has moved on
refuses the write. DesignCraft then says so. It never retries the write and never forces it. The
answer offers three ways out: reload the stored copy, save a copy of your own, or keep editing.
Leaving the Save As page without saving, or opening an ordinary Save As from inside it, abandons
that attempt (`craft_ui::cloud::cancel_save_as`), so an abandoned document and name cannot carry
into the next one.

## Where the code is

| What | Where |
|---|---|
| The pages, the menus, the badge, every string | `craft-ui`, feature `cloud` |
| What a DesignCraft document is, and the `CloudApp` answers | `crates/ui-egui/src/cloud.rs` |
| The vendor-neutral types | `craft-cloud` |
| The mock backend the tests connect | `crates/ui-egui/tests/cloud_files.rs` |

Nothing in `designcraft-ui-egui` opens a socket. That is what keeps it buildable for the web. A host
application would connect a backend; the pages state their work through it. DesignCraft holds no
provider credential and speaks no provider protocol, and none is wired in today.

## Driving it

`cloud.open` runs through the control channel and MCP, answering with the account note once a
backend is connected. See [control-protocol.md](control-protocol.md).
