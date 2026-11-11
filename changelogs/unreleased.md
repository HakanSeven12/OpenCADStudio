# OpenCADStudio - Unreleased (2026-11-08 to Present)

> **Summary:** Added a parameter order cell to the Parameters Manager, the `EATTEDIT` command, an SHX Big Font picker in Text Style settings, and corrected Big5 SHX big-font code-page detection, named `ATTSYNC`, the attribute definition card and the attribute manager's labels.

### 🌟 Notable Release Notes

* **Parameters Manager Order:** Each parameter carries a whole-number rank that every one of its rows prints and that is kept with the parameter in the drawing, so it survives saving and reopening. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **`EATTEDIT` and `ATTSYNC`:** `EATTEDIT` opens the attribute editor on a selected reference or starts the pick; `ATTSYNC <block>` synchronises a named block directly. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*

### 🚀 New Features

* **Big Font Picker:** The Text Style manager replaces the plain numeric input for big-font files with a dedicated font picker listing big fonts in the drawing, SHX files adjacent to the drawing, and SHX files in configured font folders.
* **Parameter Order:** The Parameters Manager's Order cell takes a whole-number rank for the parameter its row names; both rows of an XY or polar parameter print the same rank; an emptied cell clears it; the listed rows stay alphabetical.
* **`EATTEDIT`:** The command reaches the editor `ATTEDIT` opens, on a selected block reference or after the pick, and completes from the command line.
* **Civil 3D Objects:** TIN surfaces, alignments, profile views, profiles, corridors, sample lines and assemblies draw from the geometry their records store, each component with its own style's visibility, layer, color, linetype and lineweight. Alignments hide their masked station ranges; profile views draw their grid, geometry-point lines, axes, tangent extensions and data bands; labels take their text, attachment, offsets, rotation, ticks and leader lines from the label style, scaled by the current annotation scale. Data bands label the profiles the band names, from the alignment start; station axes also label the view's first and last station; grade labels sit half way along the straight part of each tangent and read "Vertical" on vertical ones. Drawings from Civil 3D 2013 draw their styled surfaces, alignments, profile views, assemblies and every corridor baseline. A Civil 3D object draws the display cache Civil 3D saved with it, styles applied, whenever the drawing keeps one, which brings pipes, structures, parcels, feature lines and labels drawn exactly as Civil 3D drew them; drawings that keep only a placeholder (the class name and a box at the origin) draw what is decoded from the records and nothing for classes not yet decoded. Spirals keep their shape (clothoid, Bloss, sinusoidal, biquadratic, cubic parabola, Japanese cubic, sine half-wave). Pipes draw from their records where the drawing keeps no display cache: centerline, inside and outside walls and end lines, each by its pipe style, sized from the part's inner diameter and wall thickness in drawing units, and cut back where they enter a structure. Display-cache arcs are divided finely enough that large-radius curves do not show their chords.
* **`-LINETYPE`:** The command-line form creates, renames and deletes linetypes as well as listing, loading and setting them, with quoted names for scripts.
* **Linetype Files:** `-LINETYPE EXPORT` writes linetypes to a `.lin` file, including their text and shapes; `-LINETYPE SAVE` adds one to your own library; `-LINETYPE RELOAD` picks up new `.lin` files without a restart.
* **Linetype Manager:** `LINETYPE` opens a manager in the style of the other style managers: create, copy, rename and delete linetypes, edit their dashes, gaps and dots against a live preview, and set the linetype scales and your library in one place.
* **`GROUPEDIT`:** Picks a group by one of its members or by name, then adds objects to it, removes members picked one at a time, or renames it; each edit is one undo step.
* **Group Selection Shortcut:** Ctrl+Shift+A turns group selection (`PICKSTYLE`) on and off and reports `<Groups on>` or `<Groups off>`.
* **`FILLET` in Any Plane:** Two lines that share a plane out of plan, such as a vertical one, are filleted in that plane, the arc lying in it.
* **SVG Text as Outlines:** The Plot dialog's SVG output offers Text as outlines, drawing the positioned glyph strokes and fills as vector paths so the viewer needs no installed fonts. The preference defaults off and retains searchable SVG text when disabled.

* **SVG Web-Map Strokes and Automation Options:** SVG Plot offers Non-scaling strokes for constant-width linework under downstream viewer transforms such as web-map layers; the preference defaults off and PDF output never sees it. Automation plot requests expose non-scaling strokes, scaled lineweights, shade mode, paperspace ordering, and explicit plot-style enable/disable.

### 🐛 Bug Fixes

* **Diameter Sign:** `%%c` gives the diameter sign U+2205 in the text editor, its Symbols menu and text search, the same sign drawings already showed.
* **`DIMARC` Icon:** Arc Length has its own icon instead of the Angular one.
* **TrueType Text Styles:** A style naming a TrueType family draws with that face even when an `.shx` file of the same name is in the support folder; the text editor's Style default font no longer writes a full path the editor then rejects.
* **Viewport Snaps:** Endpoint and Midpoint snap to the corners and edge midpoints of a layout viewport's frame.
* **DGN Import:** A V8 model whose first element streams are empty imports its elements instead of reporting nothing to import.
* **Rays and Xlines in Perspective:** Thin lines that pass behind the eye stay one pixel wide instead of widening towards the viewer.
* **Typed Distance Along Z:** A distance typed while tracking straight up or down goes along Z.
* **Opening Text-Heavy Drawings:** A growing glyph atlas no longer forces a second layout of every text, which doubled the scene build time of large drawings.
* **Window Zooms Over Elevated Drawings:** Zooming into a window or onto objects keeps the whole drawing in the depth range, so a surface, corridor or contours thousands of units above the window's plane no longer vanish on a close zoom.
* **Edits While Zoomed In:** Deleting or moving an object while zoomed in close no longer clips the rest of the drawing out of view.
* **PDF Underlays in Plots:** PDF underlays plot at 300 DPI instead of the screen's resolution.

* **Layout Window Plot:** A layout plot honors `area:"window"` from the request again: the window rect was being reset by the stored page setup load, so window plots over a named layout reported "plot area is empty" even with content under the rect.
* **Layout Plot:** A layout's content viewports reach the plot through the entity table's ownership even when the file's saved block-record entity lists no longer resolve (an import that renumbered handles after they were built).
* **Big Font Code Page Detection:** Removed the heuristic that forced `ansi_936` (GBK) for big-font files containing `"china"` in their name. Big5 big-fonts like `china1.shx` and `chineset.shx` (which contain trail bytes `< 0xA1`) are now correctly identified as `ansi_950` (Big5) instead of producing mojibake in viewports and exports. Legacy shape-only `chinese.shx` files alias to `chineset.shx` when available.
* **`ATTSYNC`:** A named block is synchronised directly instead of starting the interactive prompt; references rebuild from the definition in order, keep the values their tags carried, and a newly defined tag takes its default.
* **Block Editor Parameter Sets:** A box or pair set abandoned before its action steps restores the definition it started from, so a rerun does not author a second parameter.
* **Attribute Definition Editor:** Card headings use the palettes' muted label colour and the pick buttons draw a dotted square over a crosshair.
* **Automation Model Fit Default:** A Model plot request without `fit` or `scale` now uses the documented fitted Extents instead of inheriting a non-fit dialog preference. Stored paper-layout settings are unchanged.
* **Linetypes in Use:** `PURGE` keeps a linetype that a multiline style, multileader, table border or saved layer state still uses, and plugin deletes also respect block attributes; renaming a linetype updates attributes, multiline styles and saved layer states; plugins can no longer create a linetype whose name has a comma or whose pattern starts with a gap.
* **Complex Linetype Files:** A linetype with text or shapes and no description keeps them when its `.lin` file is loaded.
* **`LINETYPE SET`:** The next object drawn takes the new current linetype, and the ribbon shows it straight away.
* **Single-Loop Island Scan:** Opening a hatch whose boundary is one loop no longer runs the whole ring through the island-depth pass. A parcel boundary exported from GIS with millions of folded-back points took over two minutes per hatch in that pass while the loading bar sat at "Building scene caches…"; a single loop is already depth zero for every island style, so opening reaches the same model without it.
* **Attribute Manager:** The live toggle reads Auto preview changes; Constant shows as a disabled mode pill and the second tab reads Text Options; the properties tab lists a read-only Plot style row reporting the default.
* **Parameters Manager:** The pane opens with Expression, Value, Show and Order shown and Type and Description hidden, keeps a column chosen through its header menu across closing and reopening, and the Description column takes the width left over so its header stays whole.
* **Text Backgrounds in Plot:** MTEXT and MLEADER masks that use the drawing-window colour retain that meaning through viewport projection, so SVG/PDF output paints them paper-white instead of leaving a black rectangle behind the text. Explicitly coloured text backgrounds are unchanged.

* **Model Preview:** Direct PREVIEW and SVG/PDF export no longer carry a paper layout's fixed scale into Model when its paper-only Layout area is still selected; the inherited state now switches to centered, fitted Extents instead of placing every model object off the sheet.

* **SVG Export:** Text and font names containing code points forbidden by XML 1.0 are written with replacement characters instead of making Chrome reject the whole drawing as malformed XML.

* **Multileader Text Attachment:** Labels whose writer parked the anchor off the leader attach again, at any reading angle and along the text's own axis. Survey-exported cable-route drawings store multileader text with `text_location` a block height plus a gap away from the leader's connection; while unrotated labels snapped their attachment line onto the connection, angled ones kept the raw stored anchor and floated away. The snap now runs along the rotated text frame's up axis for angled labels, and along the reading direction for every label: the block's approach edge lands one landing gap past the connection, so labels parked half a block down the row no longer drift onto their neighbours. The row of lines a bottom-line attachment draws under the text also reaches the connection when the writer stored no landing distance, so the underlines join the leader instead of stopping a gap short. That rule now follows the actual laid-out baseline through descenders and paragraph spacing, and shares the leader's exact endpoint even at survey coordinates.
