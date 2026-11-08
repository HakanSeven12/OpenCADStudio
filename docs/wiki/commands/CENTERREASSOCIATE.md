---
summary: Attaches a center mark or centerline to a circle, arc or pair of lines that it should follow.
related: CENTERDISASSOCIATE, CENTERRESET, CENTERMARK, CENTERLINE
---

## Usage
1. Run `CENTERREASSOCIATE`.
2. At "Select a center mark or centerline to reassociate:", pick the center object.
3. For a center mark: at "Select circle or arc to associate center mark:", pick a circle or arc.
4. For a centerline: pick the first line, then the second line. Straight polyline segments also count as lines.
5. The center object moves to its new source and follows it from now on.

## Notes
- You can use it on a disassociated center object, or to move an associated one to other geometry.
- Picking the same line twice for a centerline is refused.
