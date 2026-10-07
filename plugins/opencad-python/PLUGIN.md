# Python Scripting (`opencad.python`)

## Sandboxing (Phase 1.5)

**Audited, not assumed — and the assumption was wrong.** Earlier phases
claimed "no stdlib is loaded, so `import os`/`socket`/`subprocess` is
already impossible." True for those three names, but incomplete: RustPython
0.5.0's `host_env` Cargo feature — **on by default**, and `rustpython-vm =
"0.5"` in this crate's `Cargo.toml` used to pull in the full default set —
bakes `posix`/`_ctypes` (and `os`/`nt` on other platforms) into every
interpreter's *core* module registry, entirely independent of ever adding
the separate `rustpython-stdlib` crate. Confirmed by actually running it,
not by reading feature flags: with `host_env` on, `PY_EVAL
__import__('posix').getcwd()` and `PY_EVAL open('/etc/passwd').read()` both
worked — full filesystem read (and write) access from any script.

**Fixed**: `Cargo.toml` now depends on `rustpython-vm` with
`default-features = false, features = ["compiler", "gc"]` — no `host_env`.
Re-verified the full matrix after the fix, all blocked
(`ModuleNotFoundError` or, for `open()`, `UnsupportedOperation`):
`os`, `posix`, `nt`, `subprocess`, `socket`, `ctypes`, `_ctypes`, `_socket`,
`_subprocess`, `_signal`, `pwd`, and both read and write through the
built-in `open()` — its `FileIO` backing turned out to depend on
`host_env`-gated code internally too, even though the `_io` module itself
is always registered. Confirmed no functional regression: `ocs.add_line`
and the full `examples/example.py` (including both real constraints) still
run correctly with `host_env` off.

What's left in `sys.modules` after that fix, for a fresh interpreter —
audited exhaustively, not just "fewer than before": `_ast`, `_codecs`,
`_frozen_importlib`, `_imp`, `_io`, `_thread`, `_typing`, `_warnings`,
`_weakref`, `builtins`, `codecs`, `encodings_ascii`, `encodings_utf_8`,
`ocs`, `sys` — core language/import/codec plumbing needed to run Python at
all, plus `ocs`. None of it reaches the filesystem, network, or another
process.

**One known, accepted residual gap, not silently ignored: `_thread` is
real and functional** (`_thread.start_new_thread` exists and is callable,
verified — not a stub). It's registered unconditionally in `rustpython-vm`
0.5.0's `stdlib/mod.rs` with no feature guard at all — not `host_env`, not
even its own `threading` Cargo feature — so there is no dependency-level
way to remove it short of patching RustPython itself, which is out of
scope here. It grants no filesystem/network/process access (everything
that would matter for *that* is confirmed blocked above); the realistic
risk is a script spawning a thread that busy-loops or otherwise misbehaves
inside the plugin's own process — a resource/stability concern local to
the already crash-isolated plugin process (per `plugin-architecture.md`'s
out-of-process design), not a host compromise or data exfiltration path.
Deleting `_thread` from `sys.modules` after interpreter setup was
considered and rejected as a fix: the module stays *registered* at the VM
level regardless, so a script could just re-import it — the deletion would
be security theater, not a real control.

## Commands

| Command | Description |
|---------|-------------|
| `PY_EVAL <expr>` | Evaluate one Python expression and print the result (or error) to the command line. Example: `PY_EVAL 1 + 1`. |
| `PY_RUN <path.py>` | Run a `.py` file as a script (statements, not just one expression). Reports an error the same way as `PY_EVAL` (missing file, syntax error, exception) — never a silent no-op. |

## The `ocs` module

Pre-bound as `ocs` in both commands — no `import` needed (and for `PY_EVAL`,
none is possible: it compiles in `Mode::Eval`, which only accepts a single
expression, not statements). Phases 1.2 (read), 1.3 (2D write) and part of
1.4 (`Run Script…`) of this plugin's `ROADMAP.md`.

| Function | Returns |
|----------|---------|
| `ocs.selection()` | List of the selected entities' handles (`int`), as of the last selection change — see note below. |
| `ocs.get(handle)` | Dict of `{handle, kind, layer, point}` for that entity, or `None` if it doesn't exist. `point` is `{x, y, z}` or `None` for non-point entities. |
| `ocs.get_text(handle)` | For single-line TEXT, returns its active alignment point, OCS normal, rotation, direction, and height; otherwise `None`. Reads the host's full document snapshot. |
| `ocs.text_entities()` | Lists top-level TEXT/MTEXT entities as `{handle, kind, value}` dictionaries from the active document. |
| `ocs.block_references()` | Lists top-level INSERT entities as `{handle, name, effective_name, count, is_xref, is_dynamic, visibility}` dictionaries; `count` includes MINSERT row × column multiplicity and unresolved visibility is `None`. |
| `ocs.layers()` | Lists active-document layer names, off/frozen/locked flags, and top-level entity counts. |
| `ocs.set_entity_layer(handle, layer_name)` | Moves an existing top-level entity to an existing layer, preserving its handle and layout ownership. |
| `ocs.export_layer_dwg(layer_name, output_path)` | Writes a new DWG containing one layer's top-level entities plus needed block definitions; requires an existing directory and refuses overwrite. |
| `ocs.current_layer()` | Returns the active document's current layer name (`CLAYER`). |
| `ocs.layout_for_entity(handle)` | Returns an entity's owning layout name, tab order, and paperspace sheet count; `None` when unresolved. |
| `ocs.viewport_model_outline(handle)` | Returns four XY modelspace corners and elevation for a rectangular, unclipped +Z paperspace viewport; otherwise `None`. |
| `ocs.add_closed_polyline(vertices, elevation)` | Adds a closed lightweight polyline from at least three finite `[x, y]` vertices. |
| `ocs.text_box_vertices(handle, padding)` | Returns an approximate rotated XY box for left/baseline +Z TEXT; otherwise `None`. |
| `ocs.update_closed_polyline(handle, vertices, elevation)` | Replaces a closed lightweight polyline's vertices/elevation, preserving its handle and common properties. |
| `ocs.copy_model_to_paper(handles, viewport_handle)` | Copies planar modelspace LINE/ARC/CIRCLE/LWPolyline geometry through a rectangular +Z viewport to its paperspace layout. |
| `ocs.mirror_xy_entities(handles, axis_handle, erase_source)` | Mirrors planar LINE/ARC/CIRCLE/LWPolyline entities across an existing XY LINE; optionally removes sources in the same undo group. |
| `ocs.mirror_xy_about_line(handles, point, tangent, erase_source)` | Mirrors planar curves across the XY line through a point in a tangent direction; supports curved-axis midpoint workflows. |
| `ocs.break_xy_line(handle, first, second)` | Removes the span between two interior XY points on a LINE, preserving its first fragment's handle and properties. |
| `ocs.break_xy_arc(handle, first, second)` | Removes the angular span between two interior XY ARC points, retaining the first fragment's handle. |
| `ocs.break_xy_circle(handle, first, second)` | Replaces an XY CIRCLE with the complementary ARC after removing the counterclockwise span from first to second. |
| `ocs.break_xy_polyline(handle, first, second)` | Removes a path interval from an open, straight, zero-width XY LWPolyline, keeping the first fragment's handle. |
| `ocs.align_xy_entities(handles, source, destination, source_direction, target_direction, copy_mode)` | Rigidly moves and rotates planar LINE/ARC/CIRCLE/LWPolyline entities in XY; `copy_mode=True` adds transformed copies instead of changing originals. |
| `ocs.block_insert_point(handle)` | Returns `[x, y, z]` for an INSERT, or `None`. |
| `ocs.block_bounds_xy(handle)` | Returns `[xmin, ymin, xmax, ymax]` for a simple unrotated single INSERT whose definition contains only LINE/ARC/CIRCLE/LWPolyline geometry; otherwise `None`. |
| `ocs.block_attributes(handle)` | Lists `{tag, value}` pairs for one INSERT, or `None` for another entity. |
| `ocs.set_block_attribute(handle, tag, value)` | Updates one INSERT attribute by case-insensitive tag, preserving the block's other data. |
| `ocs.set_text_value(handle, value)` | Replaces one TEXT/MTEXT entity's content, preserving other properties. Raises if the entity cannot be updated. |
| `ocs.set_mtext_mask(handle, mode, scale)` | Sets one MTEXT background mode (`off`, `mask`, or `fill`) and border scale; raises if invalid or not MTEXT. |
| `ocs.move_text(handle, x, y, z)` | Moves only a TEXT entity's active alignment point, preserving its other properties. Raises if it cannot update the entity. |
| `ocs.set_text_pose(handle, x, y, z, rotation)` | Moves and rotates one XY TEXT entity while preserving its other properties. |
| `ocs.get_area(handle)` | Returns `{area, point}` for a circle or straight, closed lightweight polyline in the world XY plane; otherwise `None`. `point` is suitable for an area-number label. |
| `ocs.get_curve_measure(handle)` | Returns `{length, point, rotation, tangent}` at half length for an XY line, circle, arc, or straight lightweight polyline; `tangent` is an XY unit vector. Otherwise `None`. |
| `ocs.line_endpoints(handle)` | Returns two `[x, y, z]` points for a nonzero XY LINE; otherwise `None`. |
| `ocs.polyline_segments(handle)` | Returns segment endpoint, width, length, and optional bulge-arc centre/radius dictionaries for an XY lightweight polyline; otherwise `None`. |
| `ocs.write_new_text_file(path, content)` | Writes a new UTF-8 report file at an explicit path during a `PY_` command. Refuses to overwrite or create parent directories. |
| `ocs.curve_samples(handle, segments)` | Returns equally spaced XY sample vertices, elevation, and closure for a line, arc, or circle; otherwise `None`. |
| `ocs.replace_with_polyline(handle, vertices, elevation, closed)` | Replaces a line, arc, or circle in place with a lightweight polyline, preserving the handle and common drawing properties. |
| `ocs.add_line(x1, y1, x2, y2)` | Adds a 2D line (z=0, layer `"0"`); returns its handle. |
| `ocs.add_circle(x, y, radius)` | Adds a 2D circle (z=0, layer `"0"`); returns its handle. |
| `ocs.add_arc(x, y, radius, start_deg, end_deg)` | Adds a 2D arc (z=0, layer `"0"`), angles in degrees; returns its handle. |
| `ocs.add_polyline(vertices, elevation)` | Adds an open 2D lightweight polyline. Each vertex is `[x, y, start_width, end_width]`; returns its handle. |
| `ocs.add_text_centered(value, x, y, z, height, rotation_radians)` | Adds middle-center aligned single-line text; returns its handle. |
| `ocs.add_text_left(value, x, y, z, height, rotation_radians)` | Adds left/baseline single-line text at its insertion point; returns its handle. |
| `ocs.add_mtext(value, point, height, width, rotation_radians)` | Adds top-left anchored MTEXT at `[x, y, z]` using Standard style; returns its handle. |
| `ocs.add_points(points)` | Adds many 3D points (each `[x, y, z]`) in one `HostApi::add_entities` batch call; returns the new handles in order. |
| `ocs.remove_entity(handle)` | Deletes one entity in the script's undo group; raises if the handle cannot be removed. |
| `ocs.add_table(rows, x, y, z, row_height, column_width)` | Adds a native table from a rectangular list of string rows; returns its handle. |
| `ocs.read_record(handle, app_name)` | Returns `{app_name, values}` for the XDATA record `app_name` on `handle`, or `None` if the entity or the record doesn't exist. See "XDATA" below for the `values` shape. |
| `ocs.write_record(handle, app_name, values)` | Attaches an XDATA record to `handle` for `app_name`, replacing any existing record for that application. Raises if `handle` doesn't exist. |
| `ocs.remove_record(handle, app_name)` | Removes the XDATA record for `app_name` from `handle`, if any. Returns `True` if a record was actually removed. |
| `ocs.command(cmd)` | Superseded. Only the `experimental-command-replay` build has it (the old fire-and-forget replay). Use `doc.command`, `doc.start_command` and `doc.modify` from the document model (see "Running OCS commands" below). |
| `ocs.select(handles)` | Replaces the selection with exactly these handles, in order (document-model build); `doc.selection = [...]` is the same. |
| `ocs.system_variable(name)` / `ocs.set_system_variable(name, value)` | Development-only `experimental-host-settings` feature. Reads or sets host-managed CLAYER (text) and SNAPANG (degrees) without nested command dispatch. Needs the host API v7 build that provides `system_variable`; absent from the portable default build. |

`add_line`/`add_circle`/`add_arc` take plain Python numbers —
`ocs.add_line(0, 0, 10, 10)` works with ints, not just floats. (Needed an
explicit fix: a bare `f64` `#[pyfunction]` parameter in RustPython, unlike
CPython's C-function argument parsing, does *not* implicitly coerce an
`int` — it raises `TypeError`. Using `rustpython_vm::function::ArgIntoFloat`
instead accepts anything with `__float__`/`__index__`, matching how a script
author actually writes coordinates.)

### Layers: `ocs.active_document.layers`

The layer table, with every change validated by the host, recorded as one undo
step, and refused (a `RuntimeError`, nothing changed) when it cannot be honoured.

```python
L = ocs.active_document.layers
L.create("Walls", color=1, lineweight=50, description="load bearing")
L.create("Grid", color=(10, 200, 30), linetype="Continuous", off=True)
L.modify("Walls", color=5, locked=True)      # only the properties given
L.rename("Walls", "Structure")               # entities on it follow
L.set_current("Grid")
L.delete("Structure", erase_objects=True)    # refused if it holds objects otherwise
"Grid" in L, L["Grid"]["color"], L.current, L.names()
```

Properties: `color` (ACI 1-255, an `(r, g, b)` tuple or a Color dict), `linetype`
(must exist in the drawing), `lineweight` (1/100 mm, 0-211; -1 ByLayer, -2 ByBlock,
-3 Default), `off`, `frozen`, `locked`, `plottable`, `transparency` (percent 0-90),
`description`. Records also carry `handle`, `entity_count` and `current`.
Refused: an existing or invalid name (empty, over 255 characters, or containing
`<>/\":;?*|=` or a backquote), a color of ByLayer/ByBlock/0, an unknown property,
renaming or deleting layer `0` or `Defpoints`, deleting or freezing the current
layer, deleting an externally referenced layer, and modifying with no properties.
Making a layer current is a setting, not an undo step.

### Text and dimension styles: `doc.text_styles`, `doc.dim_styles`

```python
S, D = ocs.active_document.text_styles, ocs.active_document.dim_styles
S.create("Title", height=5, width_factor=0.8, oblique=15, font="romans", annotative=True)
S.modify("Title", height=6)
D.create("Metric", dimscale=2, dimtxt=3.5, dimtxsty="Title")     # DimStyle field names
D.create("Metric2", copy_from="Metric", dimtxt=4)
S.rename("Title", "Heading")      # entities, dimension styles and tables follow
S.set_current("Heading"); D.set_current("Metric")
D.delete("Metric2"); S["Heading"]["height"]; D.current["name"]; S.names()
```

Same guarantees as layers: host-validated, one undo step, a refusal raises
`RuntimeError` and changes nothing (making a style current is a setting).
Text properties: `height` (0 = each text chooses), `width_factor` (0-100),
`oblique` (degrees, within 85), `font`, `big_font`, `backward`, `upside_down`,
`vertical`, `annotative`; use `font="arial.ttf"` for a TrueType face, because
the codec does not persist a separate family name. Dimension styles use the
`DimStyle` field names (`dimscale`, `dimtxt`, `dimasz`, ...); records list all
of them. Handles, `xref_*` fields and `name` are host-managed and refused, unknown
fields and wrongly typed values are refused, `dimscale` and `dimtxt` must be
positive, and `dimtxsty` must name an existing text style. `Standard` is never
renamed or deleted; a style that is current or still referenced is never deleted;
a case-only rename is refused. Style operations act on the active document.

Known persistence limits (opencadcodec, see `docs/cadcodec-reader-gaps.md`; the first two are fixed in
an open upstream pull request but OCS has not yet adopted it): a DXF
save drops the backward/upside-down flags of a text style and does not restore a
dimension style's `dimtxsty` name (the handle link survives); DWG keeps both.

### Blocks: `ocs.active_document.blocks`

```python
doc = ocs.active_document
line = doc.create_entity("Line", start=..., end=...)
circle = doc.create_entity("Circle", center=..., radius=1)
B = doc.blocks
B.create("Widget", [line, circle], base_point=(1, 1, 0), description="a widget")
doc.create_entity("Insert", block_name="Widget", insert_point={"x": 10, "y": 0, "z": 0})
B.modify("Widget", explodable=False)
B.rename("Widget", "Gadget")             # every insert follows
B["Gadget"]["entities"], B["Gadget"]["insert_count"], B.names()
B.delete("Gadget")                       # refused while an insert, style or leader uses it
```

`create` copies model or paper space entities (descriptors or handles) into a new
definition shifted by `-base_point`, so an insert at `p` puts the base point at
`p`. `erase_originals=True` removes the sources, as the BLOCK command does; the
default keeps them. No insert is placed. Refused: an empty, duplicate, invalid or
`*`-prefixed name, no entities, a missing or repeated entity, an entity inside a
block, a viewport, a non-finite base point, renaming or deleting layout,
anonymous or externally referenced blocks, and a case-only rename. Deleting also
removes the definition's contents. Records list user blocks only (`handle`,
`name`, `description`, `explodable`, `scale_uniformly`, `base_point`,
`insert_count`, `entities` as `{handle, kind}`). A DXF save drops the block
description (opencadcodec); DWG keeps it.

**Block contents.** Add to a definition with `create_entity(kind, block="Part",
...)`. It is validated exactly like a model-space entity and the host sets the
owner (so attribute definitions work: `create_entity("AttributeDefinition",
block="Part", tag=..., prompt=..., default_value=..., insertion_point=...,
height=...)`). Members are read, edited (`entity.end = (8, 0, 0)` inside
`doc.transaction(...)`) and deleted (`doc.delete_entity`) with the ordinary entity
API, using the handles in `blocks["Part"]["entities"]`. Refused: an unknown,
layout, anonymous or external block, an invalid entity, a viewport, raster image,
block marker or nested attribute, an entity on a locked layer, and an insert that
would nest a block inside itself directly or through other blocks. Adding while the
block editor is open is refused. Each add or delete is one undo step.

### Linetypes and layouts: `doc.linetypes`, `doc.layouts`

```python
doc = ocs.active_document
LT, LY = doc.linetypes, doc.layouts
LT.create("Bracket", [12, -3, 2, -3], "bracket line")   # dash 12, gap 3, dash 2, gap 3
LT.modify("Bracket", description="renamed"); LT.rename("Bracket", "Cut")
doc.layers.create("Cuts", linetype="Cut")
LT.delete("Cut")                                          # refused while a layer uses it

LY.create("Sheet1")
LY.set_page("Sheet1", paper_size=(420, 297), rotation=0, scale=(1, 50))
LY.current = "Sheet1"                                     # create_entity now draws on the sheet
doc.create_entity("Circle", center={"x": 50, "y": 50, "z": 0}, radius=5)
LY.current = "Model"
LY.rename("Sheet1", "Plan"); LY.delete("Plan")            # deletes the sheet's entities too
LY.names(), LY.current["name"], LY["Plan"]["paper_size"]
```

Both follow the same rules as the other tables: host-validated, one undo step
(switching layout is not one), a refusal raises `RuntimeError` and changes
nothing. A linetype pattern is signed lengths (positive dash, negative gap, zero
dot; 2-12 elements with a gap and a dash or dot); text and shape linetypes can be
read (`complex`) but not created or edited. `Continuous`, `ByLayer` and `ByBlock`
are never changed, names of standard linetypes are taken, renaming updates the
layers and entities that use it, and a linetype used by a layer, entity, dimension
style or the current setting is never deleted. Layouts: `Model` cannot be created,
renamed or deleted; deleting the current layout falls back to `Model`; a
case-only rename is refused. `set_page` takes the sheet in millimetres, a rotation
of 0/90/180/270 and a custom scale `(numerator, denominator)`, and updates the
layout limits. Layout operations act on the active document. Records list
`current`, `tab_order`, `entity_count`, `viewport_count`, `paper_size`, `rotation`
and `scale`. Reordering layouts and plot devices or styles are not available yet.

### Running OCS commands: `doc.command`, `doc.start_command`, `doc.modify`

Scripts can drive OCS's own tools. The host runs the real command, so it behaves
exactly as at the command line (same prompts, same geometry, same undo), and does
it synchronously on the editor thread, so a step never waits on outside input.

```python
doc = ocs.active_document
doc.command("CIRCLE 5,5 3")                 # a whole line: tokens answer the prompts, then Enter
with doc.start_command("OFFSET") as c:      # or answer prompt by prompt
    c.text(2)                               # a typed value
    c.entity(line, at=(5, 0, 0))            # an object pick
    c.point((5, 4, 0))                      # a point
    c.enter()
    print(c.outcome["status"], c.outcome["prompt"], c.outcome["added"])

M = doc.modify                              # the common tools as plain calls
M.offset(line, 2, side=(5, 5))              # returns the new entity
M.trim(target, at=(108, 0)); M.extend(target, at=(208, 0))
M.fillet(first, second, radius=2)           # radius 0 squares the corner
M.move([a, b], (0, 0, 0), (5, 5, 0)); M.copy([a], base, target)
M.rotate([a], base, 90); M.scale([a], base, 2)
M.mirror([a], p1, p2, erase_source=False); M.erase([a])
M.chamfer(first, second, 2, 3)              # distance 2 along first, 3 along second
M.array_rect([a], rows=2, columns=3, row_spacing=10, column_spacing=20)
M.array_polar([a], center, count=4, angle=360)   # counts include the original
M.explode([polyline]); M.join([line1, line2])
M.break_entity(line, first_point, second_point)
M.stretch(corner1, corner2, base, target)   # crossing window, then the displacement
M.lengthen(line, 5, at=near_the_end)        # delta; negative shortens
M.array_path([a], path_curve, count=5)      # evenly along a curve
M.array_3d([a], rows=2, columns=2, levels=2, row_spacing=10, column_spacing=20, level_spacing=30)
M.polyline_close(pl); M.polyline_open(pl); M.polyline_reverse(pl)
M.polyline_width(line_or_pl, 0.5)           # a line or arc is turned into a polyline first
M.polyline_join(pl, [line1, arc1])          # returns the merged polyline
```

Each step returns an outcome dict: `status` (`completed` or `waiting_input`),
`blocked_by`, `command`, `prompt`, `accepts` (`point`, `entity`, `selection`,
`text`, `token`, `enter`), `options` (keyword tokens), `entities`, `added`,
`unconsumed` and `error`. `c.point`, `c.text`, `c.token`, `c.entity`,
`c.selection`, `c.enter` and `c.cancel` map to those prompts; an `error` raises
`RuntimeError`, and leaving the `with` block cancels a command that is still waiting,
also closing any editor or dialog it opened. `doc.command` cancels and raises when
the line leaves a prompt open.

Selection-based tools (`move`, `copy`, `rotate`, `scale`, `mirror`, `erase`, the arrays,
`explode` and `join`) select
the entities you pass first, so the command skips its own selection prompt. Pick
points default to the middle of a line, arc or circle and the first vertex of a polyline; give `at=` for anything else.
`trim` and `extend` cut or extend against every entity in the drawing. Each command is
undone as the editor would undo it.

Guards: a command is refused while another is active (cancel it first) or when the
line names a command that could end the session, re-enter Python or change this
setting: `QUIT`, `EXIT`, `CLOSE`, `CLOSEALL`, `NEW`, `QNEW`, `OPEN`, `SAVE`, `QSAVE`,
`SAVEAS`, `SAVEALL`, `RECOVER`, `SCRIPT`, `RUNSCRIPT`, `SCRIPTCOMMANDS` and any `PY_*`.
The user can switch the whole facility off with the `SCRIPTCOMMANDS 0` command (`1`
turns it back on, plain `SCRIPTCOMMANDS` shows it); it is a saved preference, on by
default, and while it is off every `doc.command`, `start_command` and `doc.modify` call
raises `RuntimeError`. Only the command line can change it, never a script. Commands run on the active
document only. Commands that need an interactive surface a script cannot fill
(the rich-text editor, a file dialog) report it in `blocked_by` and are cancelled.
PEDIT's fit, spline, decurve and vertex-edit options are not wrapped (reach them with
`start_command("PEDIT")`); `polyline_*` return the polyline entity, which is a new
entity when the command replaced the original. Hatching has no wrapper: create it with `create_entity("Hatch", ...)`, because the
HATCH command needs an interactive boundary pick. `ocs.command_step(kind, options)`
is the raw single step behind all of this.

### Historical command-replay experiment (not in the default build)

The following records an earlier experiment and is kept for the findings, not as
instructions. It used a fire-and-forget `ocs.command()` that replayed a line through the
message loop; on OCS 2026.37 that could hang. It has been superseded by
`doc.command` / `doc.start_command` (see "Running OCS commands" above), which run the
real command synchronously through the host's `run_command` step API and were exercised
against every command tried without a hang. The prompt-sequence findings below (selection
prompts, `ALIGN`'s extra Enters) still describe how the commands behave.

**`ocs.command()` + `ocs.select()` together can apply a real, persistent
constraint, entirely from a script** — this required two host changes
(`HostApi::run_command` and `HostApi::set_selection`, `ocs_plugin_api`
v0.2.1+ built from a worktree that isn't upstream yet — see the note at the
bottom of this file), not something achievable from the plugin alone.
Verified end-to-end against a real running host, not just "it compiles":
```python
ocs.select([line1_handle, line2_handle])
ocs.command("PCONSTRAINT")
```
run as a plain `.py` file via `PY_RUN` — no automation-API help, no manual
pre-selection — ran the actual geometric solver and visibly adjusted both
lines' coordinates to be genuinely parallel (checked numerically:
direction-vector cross product ≈ 0 after, nonzero before). A real,
persistent `PCONSTRAINT` object. Same result verified for `TCONSTRAINT`
(circle-to-line tangency): center-to-line distance equals the radius
exactly after. `examples/example.py` does both — its "parallel" lines and
"tangent" circle are real constraint objects, not geometry computed once.

Why two calls, not one: constraint commands read a prior *selection* (a
`"Select objects:"` prompt), not picks fed as command-line tokens the way
`LINE`'s points are — feeding `ocs.command("PCONSTRAINT {h1} {h2}")` with
handles as trailing tokens starts the command but ends in "Command
cancelled", not a constraint. `ocs.select()` sets that selection state
first; `ocs.command()` then reads it.

**Not every command completes cleanly this way — `ALIGN` is the found
counterexample.** `ocs.command()`'s point-feeding sends exactly *one*
trailing Enter after the fed tokens, to finish the command. `ALIGN` needs
two after its point pairs (skip the optional 3rd source/destination pair,
then answer a "Scale objects? [Yes/No]" question) — so
`ocs.command("ALIGN 0,0 20,20 2,0 22,22")` **does** apply the actual
move/rotate transform correctly (verified: the entities land at the right
transformed coordinates), but leaves that Scale prompt open instead of
cleanly finishing the command. Not attempted in `examples/example.py` —
shipping something that half-completes seemed worse than leaving it out.
A general fix would need `ocs.command()` to accept a trailing-Enter *count*
(or an explicit terminator convention) instead of always exactly one;
not implemented.

**`ocs.command()` cannot invoke itself or any other plugin's command** —
only built-ins. This call is a nested plugin→host request arriving while the
plugin's own `dispatch()` is still running (its one runner thread is
blocked waiting for this exact call to return); routing it back through
normal plugin dispatch would hand that same plugin (or any plugin) a second
`Dispatch` request it has no free thread to answer, deadlocking both sides.
Found this the hard way — `ocs.command("LINE 0,0 10,10")` hung completely
before the host-side fix. `run_command` skips plugin dispatch entirely, by
design, not as an oversight.

### Which built-in commands `ocs.command()` can reach

`ocs.command()` goes through the exact same dispatcher as the interactive
command line — `HostApi::run_command` → the host's
`run_command_line_no_plugin_reentry` → `dispatch_command_inner(cmd,
allow_suggest=false, try_plugins=false)` — so almost everything typeable in
OCS also works from a script, minus one deliberate carve-out.

**Reachable: the full built-in command set**, spread across ten dispatch
families in the host's `src/app/commands/` (`fileops`, `layers`, `blocks`,
`draw`, `dim`, `inquiry`, `view`, `layerprops`, `styleprops`, `display`).
That covers everything AutoCAD-compatible the host implements natively —
drawing entities and constraints (`LINE`, `CIRCLE`, `ARC`, `POLYLINE`,
`MOVE`, `ROTATE`, `ALIGN`, `PCONSTRAINT`, `TCONSTRAINT`, in `dispatch_draw`),
dimensions (`DIMLINEAR`, `DIMALIGNED`, `DIMSTYLE`, ...), blocks (`BLOCK`,
`INSERT`, `WBLOCK`, `ATTSYNC`, ...), layers/properties (`LAYER`, `LAYISO`,
`LAYDEL`, `CHPROP`, `PROPERTIES`, ...), file ops (`NEW`, `OPEN`, `SAVE`,
`SAVEAS`, `EXPORT...`, `PLOT`, ...), inquiry (`LIST`, `DIST`, `AREA`,
`DBLIST`, ...), view (`ZOOM`, `PAN`, `VIEW`, `REGEN`, ...), and
directly-settable system variables typed as commands (`PICKBOX 5`,
`MIRRTEXT 1`, `LUPREC 4`, ...). Aliases still resolve too (`ocs.command("L
0,0 10,10")` works the same as `LINE`), since alias expansion happens before
the plugin-skip.

**Not reachable: any plugin-provided command, including this plugin's own**
(`PYTHONSHELL`, `PY_EVAL`, `PY_RUN`) — see the paragraph above for why. This
is the *only* restriction versus the interactive command line; there is no
narrower allow-list beyond it. `allow_suggest=false` also means no
autocomplete/fuzzy-matching of partial verbs (unlike typing `BAC` and
pressing Enter for `BACKGROUND`) — a script must spell out the exact command
name or a real alias.

Usage notes already verified end-to-end (see the constraint example above
and `examples/example.py`):

- Multi-point/interactive commands take their args inline on one string,
  exactly as if typed then Enter: `ocs.command("LINE 0,0 10,10")`.
- A command that reads a *prior selection* rather than inline picks
  (constraint commands are the concrete case) needs `ocs.select(handles)`
  called first — `ocs.command()` alone can't feed a "Select objects:"
  prompt.
- `ALIGN` needs two trailing Enters (skip the optional 3rd point pair, then
  answer the Scale prompt); `ocs.command()` only ever sends one, so it
  applies the transform but leaves that prompt dangling — see above.

For the exhaustive, exact list rather than this summary, the source of
truth is the `inventory::submit!(CommandRegistration { names: &[...] })`
blocks scattered across the host's `src/app/commands/*.rs` (105 files
register at least one command as of this writing); the largest single block
— everything without its own interactive command module — is
`src/app/commands/mod.rs`'s command-line autocomplete registry. Since it's
the same dispatcher, OCS's own command-line autocomplete is also a live
index of what `ocs.command()` can run.

**One `push_undo` group per script, not per `ocs.add_*` call.** A `PY_RUN`
script that calls `ocs.add_line` ten times undoes as one step. Verified
end-to-end: a two-line script undoes both lines in a single `undo`.
Implemented via `host_ctx::ensure_undo_started`, called lazily by the first
write in a given `PY_EVAL`/`PY_RUN` — a read-only script never touches the
undo stack. Note: `ocs.command()` does *not* go through this — a built-in
command manages its own undo the same way it would if typed, so `ocs.command`
calls are not folded into the script's shared undo group.

**`ocs.selection()` is a cache, not a live query.** `HostApi` has no
synchronous "give me the current selection" call — only a best-effort
`SelectionChanged` / `SelectionChangedV4` notification. It reflects the
selection as of the last time it changed, not necessarily the instant the
script calls it.

That cache is populated via `BuiltinPlugin::on_notification`, **not** by
calling `HostApi::try_recv_notification()` from `dispatch()`. Found the hard
way: `ocs_plugin_api::runner`'s own V4 event loop (`run_v4` in `runner.rs`)
drains that queue itself on every iteration and forwards each notification to
`on_notification` *before* a `Dispatch` request ever reaches `dispatch()` — so
polling it from inside `dispatch` always finds it empty. This plugin
overrides `on_notification` and stashes the latest selection in a
process-wide cache (`selection_cache.rs`); `ocs.selection()` reads that.
Verified end-to-end against a real host build: select an entity, then
`PY_EVAL ocs.selection()` returns its handle.

Each `PY_EVAL`/`PY_RUN` runs in a fresh interpreter (no stdlib, no session
state across calls) — REPL/session persistence is an open question for a
later phase.

**Typing or pasting `PY_EVAL`/`PY_RUN` directly into the interactive command
line is broken for anything beyond trivial arithmetic — a host issue, not
this plugin.** `Message::CommandInput` in `src/app/update/mod.rs` (this
repo) uppercases the *entire* input line on every keystroke *and* on paste,
and auto-submits the instant it contains a space (both deliberate, for
classic single-line CAD commands like `LINE 0,0 10,10`). That mangles case
(`ocs.add_line` → `OCS.ADD_LINE`, a `NameError`) and truncates arguments
(`PY_EVAL ` submits before you finish typing the expression) for *any*
multi-word, case-sensitive plugin argument — automation
(`ocs_execute`/`--mcp`) bypasses this UI path entirely, which is why it
never surfaced in this session's own testing until a live user tried typing
it by hand. **`Run Script…` (below) sidesteps this correctly** — the host
authors already solved the identical problem for file paths via
`ModuleEvent::PluginFileDialog`, which dispatches with original case intact.
There is no equivalent bypass for `PY_EVAL` today; typed/pasted `PY_EVAL`
only survives if it has no spaces and no case-sensitive identifiers.

## Ribbon

Two tools in the "Tools" group:

| Tool | Behavior |
|------|----------|
| Eval 1+1 | Dispatches `PY_EVAL 1 + 1` directly — a smoke test. |
| Run Script… | `ModuleEvent::PluginFileDialog` opens a native "pick a `.py` file" dialog; on selection the host dispatches `PY_RUN <chosen path>` back to the plugin. Cancel does nothing. |

**A real script manager UI (list/run/save scripts, or a dockable REPL
console) is not possible with today's `ocs_plugin_api`.** This was an open
question in `ROADMAP.md` — now answered by reading
`ribbon.rs`: `CadModule` exposes exactly one hook, `ribbon_groups()`, which
returns a **static** set of ribbon buttons/dropdowns (`RibbonItem`). There is
no API for a plugin to host a persistent custom panel, list dynamic content,
or render arbitrary UI — the ribbon vocabulary is deliberately "plain data
the host renders," not a widget toolkit. Getting a script list or REPL panel
would need a new `HostApi`/`CadModule` capability, i.e. an `ocs_plugin_api`
version bump — a host-side feature, out of scope for a plugin repo. `Run
Script…` (above) is the most a plugin can do today: reuse the host's own
native file picker via the existing `PluginFileDialog` event, which was
already there for other plugins to import files.

## AutoLISP migration

An AutoLISP-to-Python translator is a separate project and is not part of this
repository: scripts written for this plugin use the `ocs` module and the document
model described above, and a translated macro would run through `PY_RUN` like any
other script. Nothing in the plugin depends on it.

## XDATA

`ocs.read_record`/`ocs.write_record`/`ocs.remove_record` wrap
`HostApi::read_record`/`write_record`/`remove_record` — already implemented
by the host (unlike `run_command`/`set_selection`, these needed no host
change or `[patch]` entry). A record is `{"app_name": str, "values": [...]}`;
each value is `{"kind": str, "value": ...}`. Supported kinds and their
Python `value` shape:

| `kind` | Python `value` |
|---|---|
| `String`, `ControlString`, `LayerName` | `str` |
| `BinaryData` | `bytes` |
| `Handle` | `int` |
| `Point3D`, `Position3D`, `Displacement3D`, `Direction3D` | `[x, y, z]` (`float`) |
| `Real`, `Distance`, `ScaleFactor` | `float` |
| `Integer16`, `Integer32` | `int` |

```python
ocs.write_record(handle, "MYAPP", [
    {"kind": "String", "value": "part-042"},
    {"kind": "Integer32", "value": 7},
])
record = ocs.read_record(handle, "MYAPP")  # {"app_name": "MYAPP", "values": [...]}
ocs.remove_record(handle, "MYAPP")
```

This `{app_name, values}`/`{kind, value}` shape is deliberately the same one
`schoeller/ocs_python_repl` (a separate, out-of-process PyO3-based Python
plugin for OCS) uses in its `ocs.doc.read_record`/`write_record`, so a
script's XDATA handling ports between the two plugins unchanged. `write_record`
starts the script's shared undo group the same way `ocs.add_*` does (see
below); `remove_record` only starts it when a record actually existed to
remove.

## Build note: bundled host API

`Cargo.toml` uses the repository-relative `ocs_plugin_api` crate. Packaging
builds the plugin with the same compiler and CAD codec as the host and stages
matching metadata beside the library. The `experimental-command-replay` Cargo
feature keeps the old bridge code available, but the tested nested-command
path can hang. Do not enable it for production.
