---
summary: Pastes the clipboard in a format you choose: as objects, as a block, or as multiline text.
related: PASTECLIP, PASTEBLOCK, COPYCLIP, MTEXT
---

## Usage
1. Start `PASTESPEC`.
2. At **Paste as [Objects/Block/Text]**, type the format you want, or press Enter for the first one offered.
3. For **Objects** or **Block**, continue as in [`PASTECLIP`](PASTECLIP-Command) or [`PASTEBLOCK`](PASTEBLOCK-Command).
4. For **Text**, specify the insertion point of the text.

## Options
| Option | What it does |
|---|---|
| **Objects** (`O`) | Pastes the objects copied in the program, the same as `PASTECLIP`. |
| **Block** (`B`) | Pastes the copied objects as a new block, the same as `PASTEBLOCK`. |
| **Text** (`T`) | Pastes the text on the system clipboard as a multiline text object. |

## Notes
- Only the formats that are available are offered: **Objects** and **Block** need objects copied in the program; **Text** needs text on the system clipboard.
- Pasted text uses the current text style and height. Each line of the clipboard text becomes a paragraph, and the text is placed by its top-left corner.
