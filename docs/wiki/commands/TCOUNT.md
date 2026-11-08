---
summary: Numbers selected single-line text in reading order, replacing the text or adding the number before or after it.
related: TCASE, TEXT, FIND
---

## Usage
1. Select the text objects, then press Enter (skipped if you selected them before starting).
2. Enter the starting number (default 1).
3. Enter the increment (default 1).
4. Choose the placement: `[Overwrite/Prefix/Suffix] <Overwrite>`.

## Options
| Option | What it does |
|---|---|
| **Overwrite** (`O`) | Replaces each text with its number. |
| **Prefix** (`P`) | Puts the number and a space before the text. |
| **Suffix** (`S`) | Puts a space and the number after the text. |

## Notes
- Texts are numbered top to bottom, then left to right, by their insertion points.
- Only single-line text is numbered. Text on locked layers is skipped.
- The change can be undone in one step.
