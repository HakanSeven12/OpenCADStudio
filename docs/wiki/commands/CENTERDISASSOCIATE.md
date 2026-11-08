---
summary: Detaches center marks and centerlines from the objects they were created on, so they no longer follow them.
related: CENTERREASSOCIATE, CENTERRESET, CENTERMARK, CENTERLINE
---

## Usage
1. Select the center marks or centerlines, or run `CENTERDISASSOCIATE` and select them at "Select center objects to disassociate:".
2. Press Enter. The command reports how many center objects were disassociated.

## Notes
- After this, moving or changing the circle, arc or lines no longer updates the center object.
- Only center marks and centerlines are taken from the selection; other objects are ignored.
- To attach a center object again, use [`CENTERREASSOCIATE`](CENTERREASSOCIATE-Command).
