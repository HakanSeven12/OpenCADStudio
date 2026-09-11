# OpenCAD Studio User Manual

This manual describes the user-facing workflows of OpenCAD Studio. The initial
version establishes the help structure and high-level guidance; individual
commands should be expanded and verified against released behavior before the
manual is published as complete.

## Getting Started

### What OpenCAD Studio does

OpenCAD Studio is a desktop and web CAD application for opening, creating,
editing, visualizing, and saving two- and three-dimensional drawings. It works
with DWG and DXF drawings and provides command-line, ribbon, automation, and
plugin-oriented workflows.

### A first drawing

1. Create a new drawing or open an existing DWG or DXF file.
2. Confirm the active workspace, units, layer, and object-snap settings.
3. Create geometry with a drawing command such as **Line**, **Circle**, or
   **Polyline**.
4. Edit the result with grips or a modification command.
5. Save the drawing under a new name until its compatibility has been verified.

> **Data safety:** Keep an untouched copy of important third-party drawings.
> Confirm geometry, layouts, external references, and plotted output before
> replacing the original file.

### How to use this manual

Use the chapter navigation for broad workflows, the search box for a command or
concept, and the previous/next links for sequential reading. Command names,
system variables, keys, and file extensions appear in `monospaced text`.

## Files and Drawings

### Create and open drawings

Use **New** for a blank document and **Open** for an existing DWG, DXF, backup,
or autosave drawing. After opening a file from another CAD system, inspect its
units, layers, blocks, fonts, raster references, layouts, and model extents.

### Save and Save As

**Save** updates the current drawing. **Save As** changes its path, format, or
version. Prefer Save As when validating interoperability or recovering an older
drawing. A successful write does not replace visual inspection of a reopened
copy.

### Recovery and backups

OpenCAD Studio recognizes drawing backups and autosave-style files where the
underlying data is DWG or DXF. If strict loading rejects malformed entities, use
the recovery workflow and review what was discarded before continuing work.

### External resources

Raster images, underlays, fonts, materials, and external references may depend
on paths outside the drawing. Keep referenced files together when moving a
project and verify that relative paths still resolve.

## Workspace and Navigation

### Model space and paper space

Model space is the primary drawing environment. Paper-space layouts arrange
views for plotting through layout viewports. Always confirm the active space
before drawing or editing.

### Pan, zoom, and orbit

Use pan and zoom to navigate without changing drawing geometry. Use orbit and
view controls for three-dimensional inspection. **Zoom Extents** is useful when
the drawing appears empty or geometry is far from the expected location.

### Multiple model viewports

Model viewports can show different views of the same document. Grid, snap,
camera, and render mode may vary by viewport; confirm which viewport is active
before entering coordinates.

### Selection and object snaps

Selection identifies the objects a command will modify. Object snaps target
precise geometric locations such as endpoints, midpoints, centers, and
intersections. Zoom in and verify the snap marker when precision matters.

## Create and Edit Geometry

### Basic drawing commands

Create lines, arcs, circles, ellipses, polylines, splines, points, text, hatches,
and other supported entities from the ribbon or command line. Follow the active
prompt: multi-step commands remain active until they complete or are cancelled.

### Modify geometry

Common modification workflows include move, copy, rotate, scale, mirror,
offset, trim, extend, fillet, chamfer, stretch, join, explode, and erase. Locked
layers can prevent modification.

### Grips and direct manipulation

Select an entity to expose its grips. Dragging a grip previews and then commits
a geometric change. Constrained neighboring geometry may update with it.

### Undo, redo, and OOPS

**Undo** reverses recorded document operations and **Redo** reapplies them.
**OOPS** restores the most recently erased entities without reversing later
commands. Save before a risky sequence and verify that associated dimensions,
hatches, groups, and constraints were restored with their geometry.

## Layers and Properties

### Layers

Layers organize geometry and control visibility, locking, color, line type,
line weight, and plot behavior. Set the intended active layer before creating
objects. Avoid editing objects on locked layers.

### Object properties

Properties can be assigned explicitly or inherited **ByLayer** or **ByBlock**.
Use the Properties surface or commands such as `CHPROP` to inspect and change
selected objects.

### Styles

Text, dimension, multiline, table, and visual styles centralize presentation.
Changing a shared style can update many objects, so inspect affected layouts
before saving.

## Blocks and References

### Create and insert blocks

A block definition groups reusable drawing content; an INSERT places an
instance with its own position, rotation, scale, attributes, and optional array
settings. Editing a definition affects every reference to it.

### Array inserts

Array inserts place multiple block instances with row and column counts and
spacing. Large counts can generate substantial geometry. Validate scaled and
rotated arrays when exchanging drawings with another CAD system.

### Attributes and external references

Attributes attach editable values to block references. External references and
underlays retain links to other files; preserve their paths when sharing or
archiving a project.

## Annotation and Dimensions

### Text and annotation

Single-line text, multiline text, leaders, multileaders, tables, tolerances,
center marks, and centerlines communicate design intent. Missing fonts can
change layout or substitute glyphs, especially in the web build.

### Dimensions

Dimensions combine measured geometry with a dimension style. Associative
dimensions update when their source geometry changes; non-associative or
partially associated dimensions require manual verification.

### Annotation scale

Annotative objects can display differently at different viewport scales.
Confirm the active annotation scale before creating annotations and inspect all
paper-space viewports before plotting.

## Layouts and Plotting

### Layouts and viewports

Layouts represent sheets. Paper-space viewports expose model-space views at
controlled scales. Lock finished viewports to avoid accidental view changes.

### Page setup and plot preview

Choose the target device or file format, paper size, orientation, plot area,
scale, style behavior, and output quality. Use preview to catch clipping,
incorrect line weights, hidden layers, or an unintended plot area.

### Export checks

Before distributing a PDF or plotted file, compare it with the drawing at the
intended sheet size and verify text, dimensions, raster content, transparency,
line types, and viewport scales.

## 3D Modeling and Visualization

### Solids and surfaces

OpenCAD Studio can create and display solid primitives and modeled features such
as extrusions and revolutions. Imported ACIS-backed bodies are tessellated for
display while exact body data is retained where supported.

### Visual styles and lighting

Wireframe, shaded, hidden-line, materials, lights, and shadows affect display,
not the underlying object geometry. Switch to wireframe when diagnosing missing
faces or material problems.

### Tessellation quality

Curves and analytic surfaces are converted into display geometry according to
view and tolerance settings. A coarse display does not necessarily mean the
stored curve or solid is coarse.

## Parametric Constraints

### Geometric constraints

Constraints capture relationships such as coincident, horizontal, vertical,
parallel, perpendicular, equal, tangent, distance, angle, and radius. Use the
degree-of-freedom and conflict indicators to understand the sketch state.

### Editing constrained geometry

Moving or grip-editing constrained geometry can move neighboring objects. If a
solve fails or becomes redundant, inspect the constraint glyphs and conflict
status before deleting geometry.

### Persistence and history limitations

Constraint data is stored as OpenCAD Studio extension data inside DWG and DXF.
Other applications may preserve it without interpreting it. Constraint edits
and geometry changes may currently occupy adjacent history entries; verify the
result after undo, redo, erase, copy, and reopen operations.

## Commands and Shortcuts

### Command line

Enter a command name or alias, then respond to its prompts. Press `Escape` to
cancel the active command and `Return` to accept an available default. Exact
prompt options vary by command and current selection.

### Discover commands

Use command autocomplete and the ribbon to discover available operations. Some
commands are one-shot actions; others maintain state across several point,
number, keyword, or object-selection inputs.

### Shortcut conventions

Application and operating-system shortcuts can differ between native and web
builds. Avoid assigning browser-reserved shortcuts to web-only workflows.

## Automation and Plugins

### Automation interface

The automation interface can inspect documents, run commands, query geometry,
capture the viewport, and perform batches. Callers should use request IDs and
document revisions to avoid repeating or applying work to stale state.

### Plugins

Native plugins can contribute commands and ribbon surfaces through the plugin
host. Install only trusted packages that match the host API, compiler, and CAD
model compatibility requirements.

### Native and web differences

The browser build uses in-memory file workflows and cannot load native plugin
libraries. Some rendering, font, windowing, and parallel-processing behavior
also differs from the desktop build.

## Troubleshooting and Recovery

### A drawing opens incorrectly

Check units, extents, layers, layouts, block placement, missing references, and
fonts. Reopen a copy after saving to detect serialization problems early. Keep
the original file available for comparison.

### Geometry is missing or distorted

Try Zoom Extents, enable relevant layers, switch to wireframe, and inspect block
scale, array settings, clipping boundaries, object normals, and coordinates far
from the origin.

### Rendering is incomplete

Reduce unusually large images or material textures, simplify extreme hatch or
array inputs, and compare another visual style. Record the graphics backend and
the smallest drawing that reproduces the issue.

### Undo did not restore the expected state

Stop editing and save a diagnostic copy. Try the next adjacent undo entry only
when you understand what it represents. Compare the result with autosave or
backup data before overwriting the original drawing.

### Report a reproducible problem

Include the OpenCAD Studio version, platform, native or web build, exact steps,
expected and observed results, and a minimal sanitized drawing when possible.
Do not publish confidential drawings or external-reference paths.

