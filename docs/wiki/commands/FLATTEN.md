---
summary: Moves the selected objects onto the XY plane at Z=0.
related: FLATSHOT, PROPERTIES
---

## Usage
1. Select the objects, or start the command and select them at the `Select objects:` prompt.
2. Press Enter. The objects are moved to Z=0.
3. The command reports how many objects moved and how many were unchanged or not supported.

## Notes
- Supported objects: lines, arcs, circles, ellipses, polylines (2D, 3D and lightweight), splines, points, single-line and multiline text, 2D solids and 3D faces.
- Lines lose their thickness and get a straight-up normal.
- Text is only moved when it faces up (toward +Z).
- Objects that are already flat, objects of other types, and objects on locked layers are left alone.
- Only objects in the current space are used.
- The result is one undo step.
