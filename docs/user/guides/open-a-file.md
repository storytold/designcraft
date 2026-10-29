---
title: Open a DesignCraft, InDesign (IDML) or PDF file
description: Open a .designcraft document, an InDesign Markup (.idml) file or a PDF as editable objects, and what to do about missing fonts and incomplete IDML packages.
order: 20
verified: 0.7.1
---

Choose **File**, then **Open…** (Ctrl+O), and pick a `.designcraft`, `.idml` or `.pdf` file. The file opens as a new document. A PDF becomes editable paths and images (text as outlines), stacked in the same order as in the PDF.

Shortcuts on this page use Ctrl. On macOS, use Cmd instead of Ctrl.

## Steps

1. Open the **File** menu and choose **Open…**. On the start screen you can click **Open** instead.
2. In the file dialog, pick a file. The dialog lists DesignCraft, IDML, InDesign and PDF types.
3. Confirm the dialog. The document opens as a new document.

## What the Open dialog accepts

The Open dialog accepts `.designcraft`, `.idml`, InDesign `.indd`/`.indt`, and PDF (including PDF-compatible `.ai`). To place a single PDF page into an existing document without converting it, use [Place](place-images-and-text.md) instead.

IDML is InDesign Markup, the exchange format InDesign can export. Linked images are read from next to the file or from its `Links` folder. If a `Document Fonts` folder sits beside the file, DesignCraft loads those fonts first.

## Missing fonts

Text that uses a font DesignCraft cannot find is highlighted in pink. This is the missing-font highlight. Put the font files in a `Document Fonts` folder beside the file, or install the fonts on your system, so DesignCraft can find them.

## Import warnings

If an IDML package is missing some of its parts, DesignCraft still opens it and shows an **Import Warnings** alert that names each missing part. The contents of those parts are not in the document, so check your layout.

If the package has none of its spreads, there is nothing to open. DesignCraft then shows a **Can't Open the File** alert that names the missing parts.

## Save it again

**File**, then **Save** (Ctrl+S) writes a `.designcraft` file. To hand the layout back to InDesign, use **File**, then **Export IDML…**.
