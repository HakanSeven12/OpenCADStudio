# OpenCADStudio - Unreleased (2026-11-22 to Present)

> **Summary:** Faster polygon fills, polyline area calculations, and concave XCLIP hatch clipping.

### Precision input

* **Deferred TAN/PER first points.** `LINE` resolves a tangent or perpendicular picked as the first point from the second point (or from a typed `@distance<angle` direction) with a pure solver; a typed non-perpendicular direction is refused instead of being silently shortened.

### ⚡ Performance

* **Faster polygon fills, area calculations, and XCLIP.** Fill self-crossing and overlapping rings and rings with touching holes faster. Measure self-crossing polyline area and clip hatches to concave XCLIP boundaries with less geometry work. Median times fell by 16.3% to 88.8% across five Windows release benchmarks.
