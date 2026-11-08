---
summary: Edits the text of a single-line text, multiline text, attribute, dimension, tolerance or multileader in place.
related: TEXTEDITMODE, DDEDIT, TEXT, MTEXT
---

## Usage
1. The command line shows the current edit mode.
2. Select an annotation object, or type an option.
3. Edit the text in the editor that opens, then close it to keep the change.
4. In Multiple mode, select the next object. Press Enter (after at least one edit) or Esc to finish.

## Options
| Option | What it does |
|---|---|
| **Undo** (`U`) | Undoes the last edit made in this command. |
| **Mode** (`M`) | Sets the edit mode: **Single** (edit one object, then the command ends) or **Multiple** (keep picking objects until you press Enter). |

## Notes
- Single-line text and attributes open a plain in-place box; multiline text and multileaders open the rich text editor.
- Picking a leader edits the text it is attached to.
- If the object has no text, you see `TEXTEDIT: selected entity is not text.` and can pick again.
- The default mode comes from [`TEXTEDITMODE`](TEXTEDITMODE-Command).
