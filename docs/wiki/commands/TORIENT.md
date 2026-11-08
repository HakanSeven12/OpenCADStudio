---
summary: Rotates text, multiline text, attribute definitions and block attributes to an angle you give, or flips them so they read upright.
related: ROTATE, TEXT, MTEXT, ATTDEF
---
## Usage
1. Select the objects, then press Enter. You can also select them before you start.
2. At `New absolute rotation <Most Readable>:`, do one of these:
   - Type an angle, for example `45`.
   - Pick two points; the angle from the first to the second point is used.
   - Press Enter for **Most Readable**.

## Notes
- Works on single-line text, multiline text, attribute definitions, and the attributes of block references. The block itself does not rotate, only its attributes.
- **Most Readable** keeps each object's angle, but turns it by 180° when it would read upside down in the current view (including a twisted view).
- The angle you type or pick is absolute: every object gets the same rotation.
- Other objects in the selection are left unchanged. If nothing in the selection can be rotated, the command ends without a change.
