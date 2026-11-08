---
summary: Lists, adds and deletes the annotation scales of the drawing from the command line.
related: OBJECTSCALE, CANNOSCALE, ANNOSCALE
ribbon: Annotate tab › Annotation Scaling panel
---

## Usage
1. Type `SCALELISTEDIT`.
2. Type an option: **Add** (`ADD`), **Delete** (`DELETE`) or **List** (`LIST`).
3. For Add, type the new scale as a ratio such as `1:50`. For Delete, type the name of the scale to remove.

## Options
| Option | What it does |
|---|---|
| **Add** (`ADD`) | Adds a scale. The name is a paper:drawing ratio, for example `1:50`. |
| **Delete** (`DELETE`) | Removes a scale that is not current and not used by annotative objects. |
| **List** (`LIST`) | Prints every scale with its factor and how many annotative objects use it. The current scale is marked. |

## Command-line form
You can type the whole request in one line, which suits scripts:
- `SCALELISTEDIT LIST`
- `SCALELISTEDIT ADD 1:50`
- `SCALELISTEDIT DELETE 1:50`
- `SCALELISTEDIT DELETE FORCE 1:50` removes the scale even when annotative objects use it, and drops their representations at that scale.

## Notes
- The current annotation scale cannot be deleted.
- Adding or deleting a scale is one undo step.
