# Cloud storage

DesignCraft opens and saves layouts in cloud storage through **CollabCraft**, the service every
Craft app talks to. The pages, the account menu, the Share split button and the conflict modal are
`craft_ui::cloud`. All twelve apps therefore offer the same ones. What lives in this repository is
the half only DesignCraft can answer.

## Turning it on

There is no address compiled in. Set one of these before starting the app. The interface then grows
its cloud parts.

```
DESIGNCRAFT_CLOUD_URL=https://collabcraft.example designcraft
```

`CRAFT_CLOUD_URL` and `COLLABCRAFT_URL` are read as well, in that order. One setting therefore
serves the whole suite. Without any of them there is no worker. Nothing cloud-shaped is drawn
anywhere. Every `cloud.*` command answers "no cloud service is configured".

## What a user gets

- **A Cloud section on the start screen.** Your own layouts sit on one side. What other people
  shared with you sits on the other. Folders can be walked into. Each document carries an overflow
  menu holding Open, Share and Work together. File › Open from the Cloud… shows the same page over
  an open layout.
- **Save As** offers This Computer and your cloud account side by side, then a folder and a name.
- **An account menu** in the application bar. Signed out it offers to sign in. Signed in it says
  who. It offers Sign out. The badge is the account's initial.
- **A Share split button.** The label opens the share dialog. The chevron offers Work together,
  then Save a Copy, Export as PDF, Export IDML and Export EPUB.
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
answer offers three ways out: reload the stored copy, save a copy of your own, or keep editing. A
provider with no conditional writes is named on the Save page. Against such a provider a save really
can land on top of somebody else's work.

## Where the code is

| What | Where |
|---|---|
| The pages, the menus, the badge, every string | `craft-ui`, feature `cloud` |
| What a DesignCraft document is, and the `CloudApp` answers | `crates/ui-egui/src/cloud.rs` |
| The worker, the session file and the browser sign-in | `apps/designcraft/src/cloud.rs` |
| The vendor-neutral types | `craft-cloud` |
| The transport | `craft-cloud-client`, `collabcraft-share` |

Nothing in `designcraft-ui-egui` opens a socket. That is what keeps it buildable for the web. The
host application connects a backend. The pages state their work through it. DesignCraft holds no
provider credential and speaks no provider protocol. CollabCraft is the single place a provider is
configured and permissions are decided. Replacing Nextcloud with something else therefore changes
nothing in this repository.

## Driving it

The `cloud.*` commands reach all of it from the control channel and from MCP. See
[control-protocol.md](control-protocol.md).
