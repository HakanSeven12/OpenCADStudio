---
summary: Turns the vector content of a PDF underlay or PDF file into drawing objects.
related: PDFATTACH, PDFSHXTEXT, TXT2MTXT, PDFCLIP
ribbon: Insert tab › Import panel
---

## Usage
To import from a PDF underlay that is already attached:

1. Select the PDF underlay, or type **File** (`F`) to import from a file.
2. Specify the first corner of the area to import, or type an option. Press Enter to import **All**.
3. For a rectangular area, specify the opposite corner. For **Polygonal**, specify the points and press Enter.
4. Choose what happens to the underlay: **Keep**, **Detach** or **Unload**. The default is **Unload**.

To import from a file (the **File** option, or the Import panel button):

1. Choose the PDF file in the file dialog.
2. In the import dialog, choose the page, the placement (insertion point, scale, rotation) and, with **Options...**, the import settings.
3. Click **Import**. If the insertion point is set to **On screen**, specify it.

## Options
| Option | What it does |
|---|---|
| **File** (`F`) | Imports a page of a PDF file without attaching it first. |
| **Polygonal** (`P`) | Imports only what lies inside a polygon you draw. **Undo** (`U`) removes the last point. |
| **All** (`A`) | Imports the whole page. |
| **Settings** (`S`) | Opens the PDF Import Settings. |
| **Keep** (`K`) | Leaves the underlay in the drawing. |
| **Detach** (`D`) | Removes the underlay and its reference. |
| **Unload** (`U`) | Keeps the underlay reference but hides it. |

The import settings choose:
- What to import: vector geometry, solid fills, TrueType text, and raster images.
- Which layers to use: the PDF's own layers, one layer per kind of object, or the current layer.
- Whether to import as a block, join line and arc segments, turn solid fills into hatches, apply lineweights, and infer linetypes.

## Command-line form
`-PDFIMPORT` works the same way, but the **File** option asks on the command line instead of opening dialogs:

1. **Enter filename for PDF import:**
2. **Enter page number or [?] <1>:** type `?` to list the pages.
3. **Specify insertion point <0,0>:**
4. **Specify scale factor <1.0>:**
5. **Specify rotation <0>:**

It suits scripts. `PDFIMPORT` also uses these prompts when [`FILEDIA`](FILEDIA-System-Variable) is 0.

## Notes
- With the PDF layers setting, content on a PDF layer goes to a layer named `PDF_<layer name>`. Other content goes to `PDF_Geometry`, `PDF_Solid Fills`, `PDF_Text` and `PDF_Images`.
- Straight strokes that meet become polylines. Curves that are circular become arcs or circles; other curves become splines.
- Text becomes multiline text in a text style named after the PDF font.
- Raster images are saved as PNG files and attached as images.
- The import settings are kept for the rest of the session.
- Text drawn as strokes (from SHX fonts) is imported as geometry. Use [`PDFSHXTEXT`](PDFSHXTEXT-Command) to turn it back into text.
- You can undo the import.
