---
summary: Saves every open drawing that already has a file, without closing any of them.
related: SAVE, SAVEAS, CLOSEALL
web: In the web app every changed drawing that has been saved before is downloaded in one pass.
---

## Usage
1. Type `SAVEALL`.
2. Each open drawing with a file is saved to that file. The command line reports how many drawings were saved.

## Notes
- Drawings that have never been saved are skipped. The report says how many still need [`SAVEAS`](SAVEAS-Command).
- A drawing that is already being saved is skipped with a note.
- If one drawing fails to save, an error names it and the others are still saved.
- Saving is refused in a read-only session.
