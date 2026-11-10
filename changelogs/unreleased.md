# OpenCADStudio - Unreleased (2026-11-08 to Present)

> **Summary:** Added a parameter order cell to the Parameters Manager, the `EATTEDIT` command, an SHX Big Font picker in Text Style settings, and corrected Big5 SHX big-font code-page detection, named `ATTSYNC`, the attribute definition card and the attribute manager's labels.

### 🌟 Notable Release Notes

* **Parameters Manager Order:** Each parameter carries a whole-number rank that every one of its rows prints and that is kept with the parameter in the drawing, so it survives saving and reopening. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **`EATTEDIT` and `ATTSYNC`:** `EATTEDIT` opens the attribute editor on a selected reference or starts the pick; `ATTSYNC <block>` synchronises a named block directly. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*

### 🚀 New Features

* **Big Font Picker:** The Text Style manager replaces the plain numeric input for big-font files with a dedicated font picker listing big fonts in the drawing, SHX files adjacent to the drawing, and SHX files in configured font folders.
* **Parameter Order:** The Parameters Manager's Order cell takes a whole-number rank for the parameter its row names; both rows of an XY or polar parameter print the same rank; an emptied cell clears it; the listed rows stay alphabetical.
* **`EATTEDIT`:** The command reaches the editor `ATTEDIT` opens, on a selected block reference or after the pick, and completes from the command line.
* **Civil 3D Objects:** TIN surfaces, alignments, profile views, profiles, corridors, sample lines and assemblies draw from the geometry their records store, each component with its own style's visibility, layer, color, linetype and lineweight. Alignments hide their masked station ranges; profile views draw their grid, geometry-point lines, axes, tangent extensions and data bands; labels take their text, attachment, offsets, rotation, ticks and leader lines from the label style, scaled by the current annotation scale. Data bands label the profiles the band names, from the alignment start; station axes also label the view's first and last station; grade labels sit half way along the straight part of each tangent and read "Vertical" on vertical ones.
* **`-LINETYPE`:** The command-line form creates, renames and deletes linetypes as well as listing, loading and setting them, with quoted names for scripts.
* **Linetype Files:** `-LINETYPE EXPORT` writes linetypes to a `.lin` file, including their text and shapes; `-LINETYPE SAVE` adds one to your own library; `-LINETYPE RELOAD` picks up new `.lin` files without a restart.
* **Linetype Manager:** `LINETYPE` opens a manager in the style of the other style managers: create, copy, rename and delete linetypes, edit their dashes, gaps and dots against a live preview, and set the linetype scales and your library in one place.

### 🐛 Bug Fixes

* **Big Font Code Page Detection:** Removed the heuristic that forced `ansi_936` (GBK) for big-font files containing `"china"` in their name. Big5 big-fonts like `china1.shx` and `chineset.shx` (which contain trail bytes `< 0xA1`) are now correctly identified as `ansi_950` (Big5) instead of producing mojibake in viewports and exports. Legacy shape-only `chinese.shx` files alias to `chineset.shx` when available.
* **`ATTSYNC`:** A named block is synchronised directly instead of starting the interactive prompt; references rebuild from the definition in order, keep the values their tags carried, and a newly defined tag takes its default.
* **Block Editor Parameter Sets:** A box or pair set abandoned before its action steps restores the definition it started from, so a rerun does not author a second parameter.
* **Attribute Definition Editor:** Card headings use the palettes' muted label colour and the pick buttons draw a dotted square over a crosshair.
* **Linetypes in Use:** `PURGE` keeps a linetype that a multiline style, multileader, table border or saved layer state still uses, and plugin deletes also respect block attributes; renaming a linetype updates attributes, multiline styles and saved layer states; plugins can no longer create a linetype whose name has a comma or whose pattern starts with a gap.
* **Complex Linetype Files:** A linetype with text or shapes and no description keeps them when its `.lin` file is loaded.
* **`LINETYPE SET`:** The next object drawn takes the new current linetype, and the ribbon shows it straight away.
* **Single-Loop Island Scan:** Opening a hatch whose boundary is one loop no longer runs the whole ring through the island-depth pass. A parcel boundary exported from GIS with millions of folded-back points took over two minutes per hatch in that pass while the loading bar sat at "Building scene caches…"; a single loop is already depth zero for every island style, so opening reaches the same model without it.
* **Attribute Manager:** The live toggle reads Auto preview changes; Constant shows as a disabled mode pill and the second tab reads Text Options; the properties tab lists a read-only Plot style row reporting the default.
* **Parameters Manager:** The pane opens with Expression, Value, Show and Order shown and Type and Description hidden, keeps a column chosen through its header menu across closing and reopening, and the Description column takes the width left over so its header stays whole.
