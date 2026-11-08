---
summary: Rebuilds the display geometry of the drawing from the drawing data.
related: REGENALL, REDRAW, REDRAWALL
---

## Usage
1. Run the command. There are no prompts.

## Notes
- Curves, solids, meshes and other display geometry are rebuilt from the drawing data.
- Fields set to update on regeneration are updated.
- The drawing itself is not changed and no undo step is recorded.
- The command line shows "REGEN: regenerated model."
- [`REGENALL`](REGENALL-Command) does the same.
