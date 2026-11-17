---
title: Can DesignCraft open InDesign files?
description: DesignCraft imports INDD and INDT files and can open and export InDesign Markup (IDML).
order: 30
verified: 0.7.1
---

Yes. DesignCraft opens InDesign Markup (`.idml`), InDesign documents (`.indd`) and InDesign templates (`.indt`). Use **File**, then **Export IDML…** to send a layout back to InDesign.

DesignCraft converts INDD and INDT to IDML in memory before importing. Check the layout and any import warnings after opening. See [Open a DesignCraft or InDesign file](../guides/open-a-file.md).

IDML import is not complete. Open issues in the tracker report that custom text-wrap boundaries ([#451](https://github.com/storytold/designcraft/issues/451)) and saved image clipping paths ([#450](https://github.com/storytold/designcraft/issues/450)) are lost when importing IDML, and that bullets and numbers in lists show as missing-font magenta in some templates ([#110](https://github.com/storytold/designcraft/issues/110)). Check your layout after importing.

DesignCraft can also export PDF (including PDF/X-4 and PDF/A-2b), PNG and EPUB. See [Export a PDF](../guides/export-a-pdf.md).
