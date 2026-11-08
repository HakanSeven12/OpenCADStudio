---
summary: Sets the area that count mode and the Count palette count in.
related: COUNT, COUNTLIST, COUNTCLOSE
---

## Usage
1. Specify the first corner of the count area, or type an option. Press Enter for **Current area**.
2. If you picked a corner, specify the opposite corner.
3. The command line reports "count area is active."

## Options
| Option | What it does |
|---|---|
| **Current area** (`C`) | Counts in all of model space. |
| **Entire model space** (`E`) | Counts in all of model space. |
| **Object** (`O`) | Select a closed polyline as the area. It must have only straight segments and must not cross itself. |
| **Polygonal** (`P`) | Pick the corners of a polygon. Press Enter after three or more points. |

## Notes
- A rectangle or polygon is drawn as a closed polyline on the locked, non-plotting layer `0-CountArea`. It is removed when you leave count mode, unless a count field uses it.
- The **Count in a specified area** button in the Count palette and the count toolbar runs this command.
