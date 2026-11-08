---
summary: Builds a selection set that stays selected and becomes the Previous set for the next command.
related: SELECTPREVIOUS, SELECTALL, SELECTSIMILAR, QSELECT, DESELECT
---

## Usage
1. Type `SELECT`.
2. If objects are already selected, the command line reports how many were found and the command ends.
3. Otherwise select objects at the prompt, then press Enter.

## Notes
- The objects stay selected, so the next modify command uses them.
- The set is remembered as the Previous selection. [`SELECTPREVIOUS`](SELECTPREVIOUS-Command) brings it back later.
