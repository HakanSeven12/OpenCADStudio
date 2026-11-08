---
summary: Puts a wipeout mask behind selected text so lines under the text are hidden, or removes such masks.
related: WIPEOUT, DRAWORDER, TEXTTOFRONT
---

## Usage
1. Select the text objects, then press Enter (skipped if you selected them before starting).
2. At `TEXTMASK  press Enter to mask, or OFFSET <n> / OFF:`, press Enter to mask the text, or type an option.

## Options
| Option | What it does |
|---|---|
| **OFFSET** *n* | Sets how far the mask reaches past the text, in drawing units. `OFFSET` with no number shows the current value. The value is saved with the drawing. |
| **OFF** | Removes the masks from the selected text, or from all text when nothing is selected. `REMOVE` does the same. |

## Notes
- Each mask is a rectangle around the text plus the offset. Without a set offset, 15% of the text height is used.
- After masking, the text is brought to the front of the draw order so it stays readable.
- Masking a text again replaces its old mask.
- Works on single-line and multiline text. Text on locked layers is skipped.
- `-TEXTMASK` does the same as `TEXTMASK`.
