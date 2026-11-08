---
summary: Lists on the command line the layers, linetypes, text styles and blocks that are in use and so cannot be purged.
related: PURGE, AUDIT
ribbon: Manage tab › Cleanup panel
---

## Usage
1. Type `FINDNONPURGEABLE`.
2. The command line shows four lines: **Layers**, **Linetypes**, **Text styles** and **Blocks**, each with the names that objects in the drawing still use, or `(none)`.

## Notes
- It only reports. Nothing in the drawing changes.
- A name counts as in use when an object sits on that layer, uses that linetype or text style, or references that block. `ByLayer` and `ByBlock` are not listed as linetypes.
