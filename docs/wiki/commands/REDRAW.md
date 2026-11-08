---
summary: Refreshes the display of the current viewport without changing the drawing.
related: REDRAWALL, REGEN
---

## Usage
1. Run the command. There are no prompts.

## Notes
- Only the current viewport is redrawn. Use [`REDRAWALL`](REDRAWALL-Command) to redraw every viewport.
- The drawing data is not touched and no undo step is recorded. To rebuild the display geometry, use [`REGEN`](REGEN-Command).
- The command line shows "REDRAW: viewport refreshed."
