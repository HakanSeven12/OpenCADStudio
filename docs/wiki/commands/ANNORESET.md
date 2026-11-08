---
summary: Moves the other scale representations of annotative objects back to the position they have at the current annotation scale.
related: ANNOUPDATE, OBJECTSCALE, ANNOSCALE, SCALELISTEDIT
ribbon: Annotate tab › Annotation Scaling panel
---

## Usage
1. Select the annotative objects, or run the command with objects already selected.
2. Press Enter to finish the selection.

The command copies the position of the representation shown at the current annotation scale to every other scale representation of each object.

## Notes
- Only objects that have more than one annotation scale are changed. If none of the selected objects has alternate scale representations, a message says so and nothing changes.
- Objects on locked layers are skipped.
- The command line reports how many objects were synchronized. You can undo the change.
- On the ribbon the tool is labelled **Sync Scale Positions**.
