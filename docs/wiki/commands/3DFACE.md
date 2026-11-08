---
summary: Draws three- or four-sided 3D faces from points anywhere in space, with optional invisible edges.
related: EDGE, 3DMESH, 3DPOLY
---

## Usage
1. Specify the first point.
2. Specify the second point.
3. Specify the third point.
4. Specify the fourth point, or press Enter to create a three-sided face.
5. The command continues: the last edge of the face becomes the first edge of the next one. Specify the next third and fourth points to add more faces.
6. Press Enter at the third-point prompt to finish.

## Options
| Option | What it does |
|---|---|
| **Invisible** (`I`) | Type before you pick a point. The edge that starts at that point is made invisible. |

## Notes
- Each point keeps its own Z value, so a face does not have to be flat.
- Use [`EDGE`](EDGE-Command) to change edge visibility later.
