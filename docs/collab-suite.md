# The collaboration behavior suite (DesignCraft's projections)

> The shared suite every Craft app runs before its CraftHub integration is enabled (the CraftHub
> specification §18.9): each row is one automated test with a written scenario, in the family's
> behaviour-suite tradition — the suites that caught four real bugs before shipping in M9.
> This file is DesignCraft's copy of the shared catalog: the scenarios are identical in every
> app, and the *projections* column is where each app writes what the scenario means for its own
> UI.

## The scenarios

| # | Scenario | The guarantee under test | DesignCraft's projection |
|---|----------|--------------------------|--------------------------|
| 1 | Join by code, join by link | One argument that recognizes itself; the file opens in its own tab, other sessions untouched | A shared `.dcx` opens as its own window; the Pages panel stays per-document |
| 2 | Concurrent editing converges | Snapshot-hash equality after arbitrary interleavings, drops and reconnects (per seed) | The layout after convergence equals the reference render: same spreads, pages, items, threads |
| 3 | Read-only, with its reason | The reason is shown (view link, role without edit, region lock), never just a badge | The mode bar shows "view link" / "viewer role" / "locked pages" with the page range tinted |
| 4 | Batch refused with fixes | The server's refusal carries the fixes; the client applies them and lands on the server state | A frame moved onto a deleted spread snaps to the previous spread with a toast explaining why |
| 5 | Per-user undo | My steps are mine alone; later writes by others survive my undo | Undo after a peer's edit undoes only my edit; the peer's frame stays where they put it |
| 6 | Offline, then resume as the same client | The recovery copy reattaches; unsent changes replay or land as a "(recovered copy)" beside the file | The unsent page drag replays on reconnect; a conflicting copy appears in the file list |
| 7 | Cursor chat and its bounds | ≤100 characters, ephemeral, never enters the document or the journal | The bubble renders at the peer's cursor on the pasteboard, then is gone |
| 8 | Spotlight and follow | Follow tracks view and page; any view movement of mine stops the follow | The viewport animates to the presenter's spread; my own pan breaks the leash instantly |
| 9 | Presence and per-app projections | Ephemeral, throttled to change-only, bounded sizes | Cursor in spread space, the page (a presence dot on the Pages panel thumbnail), selection outlines in layer colour, viewport, the caret badge on the edited text frame |
| 10 | Region locks | Locks sync as document properties; locked regions tint; the reason is shown | A `PageRange` lock tints its pages in the Pages panel; editing on them is refused with "locked: \<label\>"; a group (`Subtree`) or property-set lock narrows the same way |
| 11 | Contribution, review, merge | The branch lives inside the live session; merge is one undo step; each side's changes show | The review panel shows both sides' property diffs, Inspect-style, per frame |
| 12 | Comments and their anchors | Anchor kinds per app; comments are entries with ids and order | DesignCraft anchors a comment to a frame or a story range (the selection); the pin rides the object |
| 13 | Background file notifications | Delivered when the window is not front; bounded, dismissible | The window badge shows "3 new" for a backgrounded shared document |
| 14 | Closing a file ends its session alone | Other sessions keep syncing behind the front window | Closing one shared document never drops another document's peers |
| 15 | The revert guard | `file.revert` is refused while its document is in a live session (including reconnect and pre-welcome) | The toast says "leave the session before reverting"; the document and its previews are intact |

## DesignCraft's adapter surface (what the suite drives)

* **Property tables** — the three synchronized object types (`designcraft-hub::props`): every
  `Item`, `Page` and `Spread` field with the compile-time guard, the kind discriminants
  (`spread`/`parentSpread`/`page` + the item kinds), and the elided `content` property (a
  group's children never ride inside it — they are their own objects).
* **Position keys** — the fractional sibling keys live beside the document
  (`DesigncraftHub`), assigned by current order at first sync, kept by the longest rising run
  under edits. Pages and items under one spread share one key space; repagination's spread
  churn comes through as honest spread deletes/creates plus page parent moves.
* **Entries** — stories (one entry per story, last-writer-wins per entry — the `textcrdt`
  upgrade is the follow-up), layers, swatches and sections, with `layerOrder`/`swatchOrder`/
  `sectionOrder` where the panel order is real state.
* **Region locks** — §6.7's document property `regionLocks`, of the `PageRange` shape
  (`designcraft-hub::locks`), held by the adapter until the model grows its own field; the
  session layer tints the covered pages and refuses covered edits with the reason.
* **Presence** — cursor in spread space, the page, the selection, the viewport and the
  text-frame caret (`designcraft-hub::presence`). DesignCraft has no timeline and no
  generation jobs: those projections stay `None` here and belong to the siblings.
* **Commands** — the unified `hub.*` catalog with the `collab.*` twins
  (`designcraft-hub::commands`): same names in every Craft app, the explicit
  lexical-unification rule.
* **Deep links** — `designcraft://join?<code\|token>&server=<url>` (`designcraft-hub::deeplink`).

## The enable rule

No app is enabled before its protocol core passes its simulation tests in CI for a full week
(nightly, fixed seeds) — and enabling happens through the app's own `crafthub` feature flag,
never through the core, so a regression is one flag flip away from off (§18.4).
