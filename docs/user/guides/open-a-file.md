---
title: Open a DesignCraft or InDesign file
description: Open a DesignCraft, IDML, INDD or INDT file and review import warnings.
order: 20
verified: 0.7.1
---

Choose **File**, then **Open…** (Ctrl+O), and pick a `.designcraft`, `.idml`, `.indd` or `.indt` file. The file opens as a new document.

Shortcuts on this page use Ctrl. On macOS, use Cmd instead of Ctrl.

## Steps

1. Open the **File** menu and choose **Open…**. On the start screen you can click **Open** instead.
2. In the file dialog, pick a file. The **DesignCraft, IDML or InDesign** filter lists all four formats. You can also filter for DesignCraft, IDML or INDD/INDT files.
3. Confirm the dialog. The document opens as a new document.

## What the Open dialog accepts

To bring an image, a PDF or a text file into a document, use [Place](place-images-and-text.md) instead.

IDML is InDesign Markup, the exchange format InDesign can export. DesignCraft converts INDD documents and INDT templates to IDML in memory before importing. Check the layout after import because some details may differ from the original. Linked images are read from next to the file or from its `Links` folder. If a `Document Fonts` folder sits beside the file, DesignCraft loads those fonts first.

## Missing fonts

Text that uses a font DesignCraft cannot find is highlighted in pink. This is the missing-font highlight. Put the font files in a `Document Fonts` folder beside the file, or install the fonts on your system, so DesignCraft can find them.

## Import warnings

For INDD and INDT files, conversion warnings appear with the IDML import warnings.

If an IDML package is missing some of its parts, DesignCraft still opens it and shows an **Import Warnings** alert that names each missing part. The contents of those parts are not in the document, so check your layout.

If the package has none of its spreads, there is nothing to open. DesignCraft then shows a **Can't Open the File** alert that names the missing parts.

## Save it again

**File**, then **Save** (Ctrl+S) writes a `.designcraft` file. To hand the layout back to InDesign, use **File**, then **Export IDML…**.
