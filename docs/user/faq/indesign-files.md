---
title: Can DesignCraft open InDesign files?
description: DesignCraft imports INDD and INDT files and can open and export InDesign Markup (IDML).
order: 30
verified: 0.7.1
---

Yes. DesignCraft opens InDesign Markup (`.idml`), InDesign documents (`.indd`) and InDesign templates (`.indt`). Use **File**, then **Export IDML…** to send a layout back to InDesign.

DesignCraft converts INDD and INDT to IDML in memory before importing. Check the layout and any import warnings after opening. See [Open a DesignCraft or InDesign file](../guides/open-a-file.md).

Check the layout after importing IDML. Some text frames may fail to render without a warning ([#193](https://github.com/storytold/designcraft/issues/193)). For pink text, see [Missing fonts](../guides/open-a-file.md#missing-fonts). Round trips still need testing across a representative set of real InDesign files.

DesignCraft can also export PDF (including PDF/X-1a:2003, PDF/X-4 and PDF/A-2b), PNG and EPUB. See [Export a PDF](../guides/export-a-pdf.md).
