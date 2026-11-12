---
summary: Pastes the clipboard in a format you choose: as objects, as a block, as multiline text, or as a table.
related: PASTECLIP, PASTEBLOCK, COPYCLIP, MTEXT, TABLE
---

## Usage
1. Start `PASTESPEC`.
2. At **Paste as**, type the format you want, or press Enter for the first one offered.
3. For **Objects** or **Block**, continue as in [`PASTECLIP`](PASTECLIP-Command) or [`PASTEBLOCK`](PASTEBLOCK-Command).
4. For **Text** or **Table**, specify the insertion point.

## Options
| Option | What it does |
|---|---|
| **Objects** (`O`) | Pastes the objects copied in the program, the same as `PASTECLIP`. |
| **Block** (`B`) | Pastes the copied objects as a new block, the same as `PASTEBLOCK`. |
| **Text** (`T`) | Pastes the text on the system clipboard as a multiline text object. |
| **Table** (`TA`) | Pastes cells copied in a spreadsheet, or a table copied in a word processor, as a table object. |

## Notes
- The prompt lists only the formats that are available: **Objects** and **Block** need objects copied in the program; **Text** needs text on the system clipboard; **Table** needs that text to be in rows of cells, as a spreadsheet copies a range.
- Pasted text uses the current text style and height. Each line of the clipboard text becomes a paragraph, and the text is placed by its top-left corner.
- A pasted table uses the current table style and the column width of [`TABLE`](TABLE-Command). Every row is a data row, with one row for each row of cells copied; a cell with several lines stays one cell. The table is placed by its top-left corner, and its cells can be edited like those of any other table.
