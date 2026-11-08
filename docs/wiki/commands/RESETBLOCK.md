---
summary: Resets selected dynamic block references to the default state of their block definition.
related: BEDIT, BVSTATE, BTESTBLOCK
---

## Usage
1. Select the dynamic block references first, or start the command and select them at the prompt `Select dynamic block references, then press Enter:`.
2. Press Enter. The command line reports how many references were reset.

## Notes
- Only dynamic block references change. Other objects in the selection are ignored.
- If any selected reference is on a locked layer, nothing is changed and an error is shown.
- The command does not run inside the Block Editor. In a Test Block window, you can reset only the single test reference.
- The reset is one undo step.
