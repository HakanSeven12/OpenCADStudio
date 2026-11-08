---
summary: Removes all geometric and dimensional constraints from the selected objects.
related: AUTOCONSTRAIN, GEOMCONSTRAINT, DIMCONSTRAINT, PARAMETERS
ribbon: Parametric tab › Manage panel
---

## Usage
1. Select the objects, or run the command with objects already selected.
2. Press Enter to finish the selection.
3. Every constraint that refers to any of the selected objects is removed. The command line reports how many constraints were deleted.

## Notes
- If no constraint refers to the selected objects, the command line shows "No constraints found."
- When a dimensional constraint is removed, its constraint dimension and its named parameter are removed too.
- The objects themselves stay in the drawing.
- In the Block Editor the command works on the constraints of the block being edited.
- You can undo the deletion with [`UNDO`](UNDO-Command).
