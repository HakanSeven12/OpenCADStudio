---
summary: Selects every visible object in the current space.
related: SA, DESELECT, SELECTSIMILAR, QSELECT
---

## Usage
1. Type `SELECTALL`.
2. The current selection is replaced. The command line reports how many objects are selected.

## Notes
- Only objects in the current space (model space, the current layout, or the block being edited) are selected.
- Objects on layers that are off or frozen, including layers frozen in the current viewport, are not selected. Neither are annotative objects hidden at the current annotation scale.
- Object types turned off in Selection Filtering are left out.
