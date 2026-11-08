---
summary: Updates annotative objects so they match the current properties of their text, dimension or multileader style.
related: ANNORESET, ANNOSCALE, OBJECTSCALE
---

## Usage
1. Select the annotation objects, or type **ALL** (`ALL`) to update every annotative object in the drawing.
2. Press Enter to finish the selection.

If objects are already selected when you start the command, they are updated at once without a prompt.

## Options
| Option | What it does |
|---|---|
| **ALL** (`ALL`) | Updates every annotative object in the drawing. |

## Notes
- Text, multiline text, attributes, dimensions and multileaders take their annotative setting and size from their style at the current annotation scale.
- Objects that are not annotative are left unchanged. The command line reports how many objects were updated and how many were skipped.
- Objects on locked layers are skipped.
- You can undo the change.
