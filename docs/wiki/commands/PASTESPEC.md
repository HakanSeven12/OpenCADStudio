---
summary: Pastes the clipboard in a format you choose: as objects, as a block, as multiline text, as a table, or as an image linked to a file or stored in the drawing.
related: PASTECLIP, PASTEBLOCK, COPYCLIP, MTEXT, TABLE, IMAGEATTACH, IMAGEEMBED
---

## Usage
1. Start `PASTESPEC`.
2. At **Paste as**, type the format you want, or press Enter for the first one offered.
3. For **Objects** or **Block**, continue as in [`PASTECLIP`](PASTECLIP-Command) or [`PASTEBLOCK`](PASTEBLOCK-Command).
4. For **Text** or **Table**, specify the insertion point.
5. For **Image** or **Embedded image**, specify the insertion point (the lower-left corner of the image) and then the width, or press Enter for the default width, as in [`IMAGEATTACH`](IMAGEATTACH-Command) and [`IMAGEEMBED`](IMAGEEMBED-Command).

## Options
| Option | What it does |
|---|---|
| **Objects** (`O`) | Pastes the objects copied in the program, the same as `PASTECLIP`. |
| **Block** (`B`) | Pastes the copied objects as a new block, the same as `PASTEBLOCK`. |
| **Text** (`T`) | Pastes the text on the system clipboard as a multiline text object. |
| **Table** (`TA`) | Pastes cells copied in a spreadsheet, or a table copied in a word processor, as a table object. |
| **Image** (`I`) | Pastes the picture on the system clipboard as an image the drawing links to. The picture is saved as a PNG file next to the drawing. |
| **Embedded image** (`E`) | Pastes the picture on the system clipboard as an image stored inside the drawing, so the drawing file travels alone. |

## Notes
- The prompt lists only the formats that are available: **Objects** and **Block** need objects copied in the program; **Text** needs text on the system clipboard; **Table** needs that text to be in rows of cells, as a spreadsheet copies a range; **Image** and **Embedded image** need a picture on the system clipboard (a screenshot, a picture copied in an image editor or a browser, or the picture a spreadsheet adds to a copied range).
- If the clipboard holds none of these, the command says so and ends.
- Pasted text uses the current text style and height. Each line of the clipboard text becomes a paragraph, and the text is placed by its top-left corner.
- A pasted table uses the current table style and the column width of [`TABLE`](TABLE-Command). Every row is a data row, with one row for each row of cells copied; a cell with several lines stays one cell. The table is placed by its top-left corner, and its cells can be edited like those of any other table.
- **Image** saves the picture in the folder of the drawing, or in the program's settings folder if the drawing has not been saved yet. If that file is moved or deleted, the drawing loses the picture; **Embedded image** has no file to lose.
