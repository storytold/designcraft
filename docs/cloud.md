# Cloud storage

DesignCraft states a contract for opening and saving layouts in cloud storage, and for sharing one.
It does not ship a provider.

## The contract

`designcraft_ui_egui::cloud` carries two traits:

- `CloudBackend`, answering the signed-in account's display name.
- `ShareBackend`, naming the service a Share control works through.

`DesignApp` holds an `Option` of each (`cloud_backend`, `share_backend`). A host application
connects one by setting the field; nothing in this crate or in the `designcraft` and
`designcraft-web` apps reaches a service directly. With neither connected, the File menu's cloud
entry is disabled and refuses, and the application bar draws no account note and no cloud-specific
Share state. `crates/ui-egui/tests/cloud_files.rs` exercises both states against an in-process mock.

## Why there is no provider here

`storytold/craft-libs#160` adds a vendor-neutral `CloudBackend`, a `ShareBackend`, and the Open,
Save and Share pages that read them, to `craft-ui`'s own `cloud` feature, together with
`craft-cloud`, the vendor-neutral file and capability types those pages hand back. That pull request
is open and unmerged. Until it lands and the workspace's craft-libs pins move to a revision that
carries it, pulling in `craft-ui`'s `cloud` feature is not possible, and no concrete backend
(CollabCraft or otherwise) lives in this repository or in its dependency graph.

Once craft-libs#160 merges, flipping the pins and wiring the apps to the real `craft_ui::cloud`
contract (in place of the local traits above) is a single commit.

## Where the code is

| What | Where |
|---|---|
| The contract (`CloudBackend`, `ShareBackend`) | `crates/ui-egui/src/cloud.rs` |
| Where the account note and the Share control are gated on a backend | `crates/ui-egui/src/chrome/app_bar.rs` |
| The File menu's `cloud.open` command | `crates/ui-egui/src/menus/{ui_commands,tables,run,state}.rs` |
| The mock backend the tests drive it through | `crates/ui-egui/tests/cloud_files.rs` |

## Driving it

`cloud.open` runs through the control channel and MCP like any other command; see
[control-protocol.md](control-protocol.md).
