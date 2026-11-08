---
summary: Selects again the set of objects that the last command started with.
related: SELECT, SELECTALL, DESELECT
---

## Usage
1. Type `SELECTPREVIOUS`.
2. The previous selection set is selected again. The command line reports how many objects are selected.

## Notes
- The set is captured each time you start a command with objects selected, and by [`SELECT`](SELECT-Command).
- If there is no previous selection, an error is shown.
