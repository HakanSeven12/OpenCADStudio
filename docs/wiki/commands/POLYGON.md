---
summary: Draws a regular closed polyline with 3 to 1024 equal sides, sized by a circle or by one edge.
related: RECTANG, PLINE, PYRAMID
ribbon: Draw tab › Draw panel (shapes drop-down)
---

## Usage
1. `Enter number of sides <6>:` type a number from 3 to 1024, or press Enter to keep the value shown.
2. `Specify center of polygon or [Edge]:` pick the center, or type `E`.
3. `Enter an option [Inscribed in circle/Circumscribed about circle] <I>:` type `I` or `C`, or press Enter for the default.
4. `Specify radius of circle:` pick a point or type a radius.

For **Edge**, pick the first endpoint of an edge, then the second endpoint.

## Options
| Option | What it does |
|---|---|
| **Edge** (`E`) | Builds the polygon from the two endpoints of one edge. |
| **Inscribed in circle** (`I`) | The vertices lie on the circle. A picked point becomes a vertex. |
| **Circumscribed about circle** (`C`) | The edges touch the circle. A picked point becomes the middle of an edge. |

## Notes
- A typed radius places the polygon with its bottom edge level. A picked point also sets the rotation.
- The number of sides and the inscribed or circumscribed choice are remembered for the next polygon.
- If you click a point at the first prompt, it is taken as the center and the current number of sides is kept.
- The ribbon drop-down offers three variants: Polygon - Inscribed, Polygon - Circumscribed and Polygon - Edge.
- The result is a closed polyline on the current drawing plane.
