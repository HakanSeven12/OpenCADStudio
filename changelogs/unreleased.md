# OpenCADStudio - Unreleased (2026-11-08 to Present)

> **Summary:** Added a parameter order cell to the Parameters Manager and the `EATTEDIT` command, and corrected named `ATTSYNC`, the attribute definition card and the attribute manager's labels.

### 🌟 Notable Release Notes

* **Parameters Manager Order:** Each parameter carries a whole-number rank that every one of its rows prints and that is kept with the parameter in the drawing, so it survives saving and reopening. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **`EATTEDIT` and `ATTSYNC`:** `EATTEDIT` opens the attribute editor on a selected reference or starts the pick; `ATTSYNC <block>` synchronises a named block directly. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*

### 🚀 New Features

* **Parameter Order:** The Parameters Manager's Order cell takes a whole-number rank for the parameter its row names; both rows of an XY or polar parameter print the same rank; an emptied cell clears it; the listed rows stay alphabetical.
* **`EATTEDIT`:** The command reaches the editor `ATTEDIT` opens, on a selected block reference or after the pick, and completes from the command line.
* **`-LINETYPE`:** The command-line form creates, renames and deletes linetypes as well as listing, loading and setting them, with quoted names for scripts.
* **Linetype Files:** `-LINETYPE EXPORT` writes linetypes to a `.lin` file, including their text and shapes; `-LINETYPE SAVE` adds one to your own library; `-LINETYPE RELOAD` picks up new `.lin` files without a restart.
* **Linetype Manager:** `LINETYPE` opens a manager in the style of the other style managers: create, copy, rename and delete linetypes, edit their dashes, gaps and dots against a live preview, and set the linetype scales and your library in one place.

### 🐛 Bug Fixes

* **`ATTSYNC`:** A named block is synchronised directly instead of starting the interactive prompt; references rebuild from the definition in order, keep the values their tags carried, and a newly defined tag takes its default.
* **Block Editor Parameter Sets:** A box or pair set abandoned before its action steps restores the definition it started from, so a rerun does not author a second parameter.
* **Attribute Definition Editor:** Card headings use the palettes' muted label colour and the pick buttons draw a dotted square over a crosshair.
* **Linetypes in Use:** `PURGE` keeps a linetype that a multiline style, multileader, table border or saved layer state still uses, and plugin deletes also respect block attributes; renaming a linetype updates attributes, multiline styles and saved layer states; plugins can no longer create a linetype whose name has a comma or whose pattern starts with a gap.
* **Complex Linetype Files:** A linetype with text or shapes and no description keeps them when its `.lin` file is loaded.
* **`LINETYPE SET`:** The next object drawn takes the new current linetype, and the ribbon shows it straight away.
* **Attribute Manager:** The live toggle reads Auto preview changes; Constant shows as a disabled mode pill and the second tab reads Text Options; the properties tab lists a read-only Plot style row reporting the default.
