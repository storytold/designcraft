---
title: Place images, PDFs and text
description: Bring images, PDF pages, Word and plain text files into a DesignCraft document with File, Place.
order: 30
verified: 0.7.1
---

Choose **File**, then **Place…** (Ctrl+D), pick a file, and DesignCraft puts it into your document.

Shortcuts on this page use Ctrl. On macOS, use Cmd instead of Ctrl.

## Steps

1. Open the **File** menu and choose **Place…**.
2. Pick a file. The dialog's **Graphics and text** filter covers the file types below.
3. For an image, DesignCraft places it in the selected empty frame if there is one. The image fills the frame once and keeps its proportions. **Auto-Fit** stays off, so resizing the frame later crops the image instead of scaling it.
4. For a text file, it goes into the insertion point, the selected frame, or a new frame.

## File types

| Kind     | Extensions                                                                      |
|----------|---------------------------------------------------------------------------------|
| Images   | png, jpg, jpeg, gif, webp, tif, tiff, bmp, psd, svg                             |
| Vector   | pdf, ai, eps                                                                    |
| Text     | txt, docx, rtf, md, icml (InCopy story)                                         |
| Table    | xlsx (placed as a table)                                                        |
| Layout   | idml, designcraft (one page's objects, placed as a group)                       |
| Media    | mp4, m4v, mov, webm, mp3, m4a, wav, ogg (placed in a media frame)               |

The same dialog has separate filters for **Graphics**, **Text (Word, RTF, plain, InCopy, Excel)** and **Video and sound**.

## Place a page of a PDF

1. Choose **File**, then **Place…** and pick a PDF with more than one page.
2. In the **Place PDF** dialog, type the page number in **Page**. The label shows the page count.
3. Pick a box in **Crop to:**. The choices are **Crop**, **Trim**, **Bleed**, **Art** and **Media**.
4. Click **OK**.

A PDF with one page is placed without this dialog.

## Place Word or RTF text with your styles

**File**, then **Place with Import Options…** (Ctrl+Shift+D) opens an **Import Options** dialog for `.docx` and `.rtf` files, where you can map the file's styles to your document's styles before the text lands. For other file types it places the file straight away.

## Place an InCopy story

An `.icml` file is a story saved by InDesign or InCopy (**File**, **Export**, **InCopy Document**), or written by a converter such as Pandoc. Place it like any text file. Its paragraph and character styles, swatches, footnotes, tables and inline images come with it. Styles your document already has keep your definitions. Images the story links to are looked for in the file's folder.

To write a story as an InCopy story, click in it, choose **File**, then **Export Text…**, and give the file name the `.icml` extension.

## If a file cannot be placed

DesignCraft shows an alert when a file cannot be opened or placed. Ask in [Discord](https://discord.gg/artcraft) or see [Report a problem](../troubleshooting/get-help.md).
