---
summary: Joins lines, arcs, open polylines and open splines that meet end to end into one object.
related: PEDIT, BREAK, BREAKATPOINT, EXPLODE
ribbon: Draw tab › Modify panel
---
## Usage
1. At `Select source object or multiple objects to join at once:`, select objects.
   - Select several objects and press Enter to join them all together.
   - Select one object and press Enter to use it as the source.
2. With a source, select the objects to join to it and press Enter.

If two or more objects are already selected when you run the command, they are joined at once.

## Options
| Option | What it does |
|---|---|
| **Close** (`L`) | Offered when the source is an arc. Turns the arc into a full circle. |

## Notes
- The source can be a line, an arc, an open polyline or an open spline. A closed polyline or spline cannot be a source.
- Objects that meet end to end become one object:
  - collinear lines become one line;
  - arcs of the same circle become one arc, or a circle when they close it;
  - other flat chains become a polyline, with arcs kept as arc segments;
  - chains that change height become a 3D polyline.
- Across a gap, only collinear lines and arcs of the same circle are joined.
- When joining to a source, the result keeps the source's properties.
- If nothing can be joined, the command line explains why: the objects do not touch, branch, leave one plane, or are of a type that cannot be joined.
