---
summary: Extends lines, arcs, open polylines and other open curves to meet boundary edges.
related: TRIM, EXTRIM, FILLET, LENGTHEN
ribbon: Draw tab › Modify panel
---

## Usage
1. Type `EXTEND`.
2. At `Click near an end or draw a crossing box to extend multiple objects (Shift trims):`, click an object near the end you want to extend. The end grows to the nearest boundary.
3. Keep clicking objects, or drag a box to extend several at once.
4. Press Enter to finish.

In Standard mode the command first asks you to `Select boundary edges`. Pick the edges (pick one again to remove it), press Enter, then click the objects to extend.

## Options
| Option | What it does |
|---|---|
| **Boundary edges** (`B`) | Choose the boundary edges yourself. Only those edges stop the extension. |
| **Fence** (`F`) | Pick points of a fence line. Press Enter to extend every object the fence crosses. |
| **Crossing** (`C`) | Pick two corners. Every object the box crosses is extended. |
| **Edge** (`E`) | Standard mode only. Switches between extending to where a boundary would be if it were extended (**Edge: Extend**) and to the boundary as drawn (**Edge: No extend**). |
| **mOde** (`O`) | Choose **Quick** or **Standard**. Quick uses every object as a boundary. Standard asks for boundary edges first. |

## Notes
- Hold Shift while you click to trim the object instead of extending it.
- If you select objects before starting `EXTEND`, they are used as the boundary edges.
- The mode you pick is remembered for both `EXTEND` and [`TRIM`](TRIM-Command) (the `TRIMEXTENDMODE` setting).
- You can extend lines, arcs, elliptical arcs, open lightweight polylines and splines. Lines, arcs, circles, ellipses, rays, construction lines, splines and polylines can act as boundaries.
- Objects on locked layers cannot be changed.
