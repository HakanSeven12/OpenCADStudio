---
summary: Edits the text of a table cell that you pick.
related: TABLE, TABLESTYLE, TEXTEDIT
---

## Usage
1. Select a table cell.
2. Type the cell text in the editor that opens over the cell.

## Command-line form
`-TABLEDIT` asks for the cell text at the command line instead of opening the editor:
1. Select a table cell.
2. `Enter text for [row,column] <current text>:` Type the new text and press Enter. Type `/n` for a line break.

This form suits scripts. In a running script, `TABLEDIT` behaves the same way.

## Notes
- A locked or read-only cell cannot be edited; you see "The cell is content locked." and can pick again.
- If you miss a cell, you see "No editable table cell picked." and can pick again.
- Press Enter at the pick prompt to end the command.
- Double-clicking a table cell opens the same editor.
