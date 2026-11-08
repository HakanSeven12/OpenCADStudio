---
summary: Explodes the selected objects and turns the attribute values of block references into plain text.
related: EXPLODE, ATTSYNC, BATTMAN
---

## Usage
1. Select the objects, or run `BURST` and then select them.
2. Press Enter. Each object is broken into its parts, and the command reports how many objects were burst.

## Notes
- For a block reference, each attribute value becomes a text object (a multi-line attribute becomes multiline text) at the same place, with the same style. Constant attributes also become text. Unlike [`EXPLODE`](EXPLODE-Command), the attribute definitions do not reappear.
- Parts on layer 0 move to the reference's layer, and ByBlock colour, linetype, lineweight and transparency take the reference's values.
- Objects on locked layers are skipped.
- With [`EXPLMODE`](EXPLMODE-System-Variable) set to 0, block references that are not scaled uniformly and positively are left as they are.
- If nothing in the selection can be exploded, the command says so and changes nothing.
