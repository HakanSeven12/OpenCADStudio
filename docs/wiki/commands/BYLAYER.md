---
summary: Sets the current colour for new objects to ByLayer.
related: COLOR, SETBYLAYER, LAYER
---

## Usage
1. Run `BYLAYER`. There are no prompts.
2. New objects you draw from now on take their colour from their layer. The command line confirms "Object colour set to ByLayer."

## Notes
- Existing objects are not changed. To reset the properties of existing objects to ByLayer, use [`SETBYLAYER`](SETBYLAYER-Command).
- Only the colour is changed; the current linetype and lineweight stay as they are.
- To pick another current colour, use [`COLOR`](COLOR-Command).
