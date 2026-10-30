# TRIM: remove a hatch region

This extends native HATCH trimming to multi-edge arrangements: the hatch fill
is partitioned into bounded cells, and only the cell under the pick is removed.
The implementation uses exact boundary sources where available, reconstructs
analytic curves, and refuses edits whose retained topology cannot be verified.

In Quick mode, run `TRIM` (`TR`) and click **inside the filled region to remove**.
A click between pattern strokes works too. For example, a line joining the top
and bottom of a hatched rectangle divides its fill into two regions: clicking
on the right removes only the right-hand fill. The rectangle and dividing line
remain unchanged.

In Standard mode, select the cutting edges and press Enter first. Preselected
cutting edges work as usual; the hatch's own stored outline always bounds its
fill even if that outline is not a separate selected object. Hover previews
show the removed boundary in gray and the surviving boundary in cyan. The
command stays active for more clicks; Enter finishes. Undo/Redo restores each
edit, including the original hatch and its metadata.

- The surviving object remains a HATCH with its existing identity, layer,
  color, transparency, resolved pattern, angle, scale and pattern origin.
- Cutting edges must partition the filled region. An unpartitioned fill,
  dangling line, pick outside the fill/in a hole, or ambiguous pick on a
  boundary is a no-op. No nearest-region fallback erases another region.
- Coplanar lines, polylines and curves are supported. Simple heavy/3D polyline
  cutters contribute only constituent segments in the hatch plane; mesh,
  polyface, curve-fit and spline-fit modes are ignored rather than approximated
  from their vertex order. Rays/XLines are bounded
  to the hatch extent; Standard mode's Edge: Extend extrapolates line limits.
- Existing holes and disconnected fill components are retained. New paths
  recover analytic arcs/ellipses/splines from the original source geometry.
- The changed hatch becomes **non-associative**: its former complete outline
  no longer describes it. This prevents later boundary edits from restoring
  the removed fill. Island style is retained unless the new holes/islands
  require Normal style to represent the result.
- This is a direct interior-click operation, not a new Fence/Crossing hatch
  operation. It does not edit hatch content inside a block reference or
  MPOLYGON, and Shift+click retains the existing curve-extension behavior.

Implementation: `src/modules/draw/modify/trim_hatch_region.rs`. It builds a local
planar arrangement in the hatch OCS, classifies filled cells, removes the
clicked cell and cancels shared surviving half-edges before reconstructing
exact boundary paths. Pattern rendering strokes are never exploded.

One-to-one command replacements use `replace_command_entity` /
`Scene::update_entity`, not erase/add with the same handle: the latter
would duplicate owning-block membership and produce duplicate DXF records
that flat entity queries do not reveal. Tests cover both block membership
and serialization before and after Undo/Redo.

Regression tests:

```sh
cargo test --release --locked --lib trim_hatch -- --test-threads=1
```
