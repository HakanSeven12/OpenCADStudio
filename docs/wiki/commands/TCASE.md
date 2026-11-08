---
summary: Changes the letter case of selected single-line and multiline text.
related: TEXTEDIT, FIND, TCOUNT
---

## Usage
1. Select the text objects, then press Enter (skipped if you selected them before starting).
2. Choose the case: `TCASE [Upper / Lower / Sentence / Title]:`

## Options
| Option | What it does |
|---|---|
| **Upper** (`U`) | ALL CAPITALS. |
| **Lower** (`L`) | all lowercase. |
| **Sentence** (`S`) | Capital first letter of each sentence. |
| **Title** (`T`) | Capital First Letter Of Each Word. |

## Notes
- Only single-line text and multiline text are changed; other selected objects are ignored.
- Objects on locked layers are skipped.
- The command line reports how many objects were updated. The change can be undone in one step.
