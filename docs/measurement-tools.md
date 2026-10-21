# Measurement Tools

OpenCADStudio provides interactive tools for measuring distances, radii, angles, and areas. Measurement results appear near the cursor during the operation and remain briefly visible at the completion point. Measurements do not create drawing entities.

## Commands

| Tool | Command | Description |
| --- | --- | --- |
| Distance | `DIST` | Measure the distance between two points. |
| Radius | `MEASURERADIUS` | Measure the radius and diameter of a circle or circular arc. |
| Angle | `MEASUREANGLE` | Measure an angle using three points or two LINE entities. |
| Area | `AREA` | Measure areas and perimeters, including accumulated areas. |

The **Measure** dropdown lists **Distance**, **Radius**, **Angle**, and **Area**, in that order. Each tool can be started from the menu or by entering its command. `MEASUREGEOM` and its `MEA` alias offer the same four modes plus **Quick**.

### Quick — `MEASUREGEOM` → `Q`

As the cursor moves, the nearest geometry along the working plane's X and Y axes is measured live: cyan measurement lines and a Horizontal/Vertical readout appear by the cursor. A click prints the current readout to the command line and measuring continues; Enter ends the command. Angles where the lines meet slanted geometry are not shown yet.

## Using the measurement tools

### Distance — `DIST`

1. Specify the first point.
2. Move the cursor to preview the current distance and geometry.
3. Specify the second point to complete the measurement.

Object snaps remain available. Length values use the drawing's existing length-formatting settings.

### Snapping to regions

A `REGION` exposes its outer and inner boundaries to ordinary object snaps (F3),
without requiring EXPLODE or enabling 3D object snaps (F4). Use these references
when specifying points for lines, dimensions, or measurements.

Boundary pieces use the same snap modes as ordinary lines, arcs, circles,
ellipses, and splines: enable the modes appropriate to the geometry. For example,
a circular boundary provides Center, Quadrant, and Tangent, not artificial
endpoints along its display tessellation. Holes and inclined regions retain their
own boundary coordinates. Region rendering, selection, and plotted output are
unchanged. This does not add persistent dimension associativity to region edges.

### Radius — `MEASURERADIUS`

1. Move the cursor over a circle or circular arc to preview its radius and diameter.
2. Select the entity to complete the measurement.

Other entity types and invalid radii are rejected; the command stays active so another entity can be selected. Moving off a valid entity clears its preview rather than retaining a stale value.

### Angle — `MEASUREANGLE`

The command provides two selection methods:

**Three points (default)**

1. Specify the vertex.
2. Specify a point on the first ray.
3. Specify a point on the second ray.

The result is an unsigned angle in degrees, from 0° to 180°. Zero-length rays and invalid coordinates are rejected. The number of decimal places follows `AUPREC`.

**Two lines**

1. At the initial prompt, choose **Lines** (`L`).
2. Select the first LINE entity.
3. Select the second LINE entity.

Before selecting the first line, choose **Points** (`P`) to return to the three-point method. The line-based result is the smaller angle between the line directions, from 0° to 90°, regardless of endpoint order. Selecting the same entity twice, selecting a non-LINE entity, or selecting a zero-length line does not produce a valid result.

### Area — `AREA`

The area tool supports point-based measurement, object selection, and **Add/Subtract** accumulation. Its live preview shows the current area and, when accumulating, the provisional total. The total changes only when the current measurement is accepted.

For object selection, the area preview is recalculated when the selected entity changes rather than reintegrating complex curves every frame. Area and perimeter values use the application's existing formatting rules.

## Interactive results

- While a measurement is in progress, a result label appears near the cursor. The label is available independently of the `DYN` setting.
- Once a measurement is completed, the final label remains at the completion position instead of following subsequent cursor movement.
- The next left, middle, or right mouse-button press in the drawing area dismisses the final label. The original mouse event continues to normal handling, so the click can still select an entity or begin another operation.
- `Esc` and starting a new command also dismiss the final label. Releasing the click that completed a measurement does not immediately dismiss it.
- Labels follow the current theme, avoid the viewport edges and command-line area, and do not capture mouse input.
- Labels belong to their document tab: switching documents does not display another tab's result.
- Measurement labels are temporary UI overlays, not DWG entities. They do not change entity counts, mark an otherwise unchanged drawing as modified, or appear in saved/printed drawings. Command-line results remain available.

## Examples and manual checks

These examples are useful when verifying measurement behavior after a change:

| Tool | Input | Expected result |
| --- | --- | --- |
| `DIST` | `(0,0,0)` to `(3,4,0)` | Distance: 5 drawing units |
| `MEASURERADIUS` | Circle or arc with radius 10 | Radius: 10; diameter: 20 |
| `MEASUREANGLE` (points) | Vertex `(0,0,0)`, rays through `(1,0,0)` and `(0,1,0)` | 90° |
| `MEASUREANGLE` (points) | Vertex `(0,0,0)`, rays through `(1,0,0)` and `(-1,1,0)` | 135° |
| `MEASUREANGLE` (lines) | Perpendicular LINE entities | 90° |
| `MEASUREANGLE` (lines) | Parallel LINE entities | 0° |
| `AREA` | Triangle `(0,0,0)`, `(4,0,0)`, `(0,3,0)` | Area: 6; perimeter: 12 |

Additional recommended checks:

- Verify all four Measure menu entries, command-line commands, `MEASUREGEOM`, and `MEA`.
- Check live previews with object snaps, `DYN` on and off, different precision settings, and large coordinates.
- Reject invalid radius selections, repeated lines, zero-length rays, and unsupported entity types without ending the command unexpectedly.
- Verify `AREA` object selection and **Add/Subtract** with multiple regions.
- After finishing a measurement, move the cursor, click through the result label, press `Esc`, and switch between document tabs.
- Check both light and dark themes, viewport corners, and an open command line.
- Verify that measurement alone leaves the drawing unchanged and that result labels are not persisted when saving and reopening a DWG.

## Known limitations

- Two-line angle measurement returns the smaller direction angle (0°–90°), not an intersection-specific or reflex angle. The same direction angle is reported for skew lines in 3D; no intersection is calculated.
- Three-point angle measurement returns an unsigned angle (0°–180°), not a reflex angle.
- The existing area semantics of `AREA` and the Area mode of `MEASUREGEOM` differ: `AREA` uses its existing spatial/curve-based computation, while the `MEASUREGEOM` Area mode uses a polygon projected onto the working plane. Results may therefore differ for non-coplanar points.
- Result labels are screen-space overlays, not persistent dimensions. They cannot be edited as drawing objects or printed.
- Newly added interface strings in some locale catalogs use English fallbacks until translated.

## Developer notes and tests

Radius and angle commands use separate initial modes of the existing `MeasureGeomCommand` and share selection and measurement logic. Angle calculations use `f64` vectors, scaled normalization, and `atan2` for numeric robustness. The interactive measurement label is managed centrally on a per-document basis rather than being stored in the drawing model.

Relevant Rust tests cover geometry, invalid selections, numeric precision, area previews and accumulation, command registration, document immutability, overlay lifecycle, and viewport event forwarding. To run the targeted suites from the repository root:

```sh
cargo test --locked --lib measurement -- --test-threads=1
cargo test --locked --lib modules::draw::inquiry -- --test-threads=1
```

These commands are development checks; they do not replace manual UI testing.
