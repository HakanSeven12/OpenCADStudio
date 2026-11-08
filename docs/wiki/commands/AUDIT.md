---
summary: Checks the drawing for errors in its internal structure and, if you ask, repairs them.
related: RECOVER, PURGE, OVERKILL
ribbon: Manage tab › Cleanup panel
---

## Usage
1. Fix any errors detected? [Yes / No] <No>: type **Yes** (`Y`) to repair, or press Enter to only check.

The command line shows how many objects were scanned and how many errors and warnings were found, followed by the first 50 problems.

You can also type the answer on the same line, for example `AUDIT Y`.

## Options
| Option | What it does |
|---|---|
| **Yes** (`YES`) | Checks the drawing and repairs what it can. The command line reports how many repairs were applied and how many problems are left. |
| **No** (`NO`) | Only checks the drawing and reports the problems. Nothing is changed. |

## Notes
- If no problems are found, the command line says so.
- The repair is one undo step.
