---
title: Export a PDF
description: Export your DesignCraft document as a PDF for desktop printing, a print shop, screen or archive.
order: 40
verified: 0.7.1
---

Choose **File**, then **Export PDF…**, pick a **Preset**, click **OK** and name the file.

Shortcuts on this page use Ctrl. On macOS, use Cmd instead of Ctrl.

## Steps

1. Open the **File** menu and choose **Export PDF…**.
2. In the **Export PDF** dialog, pick a **Preset:** (see the table below).
3. Under **General**, leave **Page Range:** as `all`, or type the pages you want, for example `1-3,5`. **Spreads** is a checkbox in the same section.
4. Under **Compression**, tick or clear **Compress images**.
5. Under **Marks and Bleeds**, tick **Include document bleed**, **Crop Marks**, **Bleed Marks** or **Page Information** as your printer asks. **Weight:** and **Offset:** set the marks.
6. Under **Advanced**, pick a **Standard:** (**None**, **PDF/X-4** or **PDF/A-2b**) and decide on **Tagged PDF**.
7. Click **OK**, then choose where to save. The file name starts as your document's title with `.pdf`.

## Presets

Picking a preset fills in the other settings. Changing any setting by hand switches the preset to **Custom**.

| Preset              | Standard | Compress images | Crop marks | Bleed marks |
|---------------------|----------|-----------------|------------|-------------|
| Desktop Printing    | None     | off             | off        | off         |
| Commercial Printing | None     | on              | on         | on          |
| Screen and Email    | None     | on              | off        | off         |
| PDF/X-4             | PDF/X-4  | off             | on         | on          |
| PDF/A-2b            | PDF/A-2b | off             | off        | off         |

**Screen and Email** also sets the transparency flattener to low resolution, which draws pages with transparency at 72 ppi. DesignCraft remembers your last settings for next time.

## Other exports

The **File** menu also has **Export Page as PNG…** (Ctrl+E), **Export IDML…**, **Export EPUB…**, **Export EPUB (Fixed Layout)…**, **Export PDF (Interactive)…**, **Export HTML…**, **Export XML…**, **Export Text…** and **Print Booklet…**.
