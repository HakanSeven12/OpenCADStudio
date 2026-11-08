---
summary: Ends the sketch started with CREATESKETCH and restores the UCS and snap setting it changed.
related: CREATESKETCH, EXTRUDE, BROWSER
---

## Usage
1. Draw your profile while a sketch is open.
2. Type `FINISHSKETCH`, or click the open sketch in the Browser panel.
3. The previous UCS and snap setting come back. The command reports how many objects you drew.

## Notes
- The objects drawn in the sketch become the Previous selection set. Start [`EXTRUDE`](EXTRUDE-Command) and type `P` at the selection prompt to use them.
- If no sketch is open, you see `No sketch is open.`
