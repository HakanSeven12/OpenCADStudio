---
summary: Sends every hatch in the current space behind all other objects.
related: DRAWORDER, TEXTTOBACK, HATCH, HATCHEDIT
---

## Usage
Run the command. It needs no input.

All hatches in the current space move behind the other objects, and the command line reports how many were moved.

## Notes
- Hatches keep their order among themselves.
- Hatches on locked layers are not moved.
- If there are no hatches, the command says so and does nothing.
- The current selection is left as it was. The change can be undone.
