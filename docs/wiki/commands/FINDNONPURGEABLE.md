---
summary: Lists on the command line the named items that PURGE leaves behind, by kind.
related: PURGE, AUDIT
ribbon: Manage tab › Cleanup panel
---

## Usage
1. Type `FINDNONPURGEABLE`.
2. The command line shows one line per kind that has such items: layers, linetypes, text styles, blocks, dimension, multileader, table and multiline styles, and materials, each with the names [`PURGE`](PURGE-Command) would not remove.

## Notes
- It only reports. Nothing in the drawing changes.
- It uses the same rules as PURGE, so an item is listed when something uses it, when it is current, or when it always stays (such as layer 0, the Standard styles and `Continuous`).
- An item used only by a block that is itself unused is listed too: PURGE removes the block first and the item on its next pass.
