# OpenCADStudio - Unreleased (2026-11-08 to Present)

> **Summary:** Added a parameter order cell to the Parameters Manager and the `EATTEDIT` command, and corrected named `ATTSYNC`, the attribute definition card and the attribute manager's labels.

### 🌟 Notable Release Notes

* **Parameters Manager Order:** Each parameter carries a whole-number rank that every one of its rows prints and that is kept with the parameter in the drawing, so it survives saving and reopening. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **`EATTEDIT` and `ATTSYNC`:** `EATTEDIT` opens the attribute editor on a selected reference or starts the pick; `ATTSYNC <block>` synchronises a named block directly. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*

### 🚀 New Features

* **Parameter Order:** The Parameters Manager's Order cell takes a whole-number rank for the parameter its row names; both rows of an XY or polar parameter print the same rank; an emptied cell clears it; the listed rows stay alphabetical.
* **`EATTEDIT`:** The command reaches the editor `ATTEDIT` opens, on a selected block reference or after the pick, and completes from the command line.

### 🐛 Bug Fixes

* **`ATTSYNC`:** A named block is synchronised directly instead of starting the interactive prompt; references rebuild from the definition in order, keep the values their tags carried, and a newly defined tag takes its default.
* **Block Editor Parameter Sets:** A box or pair set abandoned before its action steps restores the definition it started from, so a rerun does not author a second parameter.
* **Attribute Definition Editor:** Card headings use the palettes' muted label colour and the pick buttons draw a dotted square over a crosshair.
* **Attribute Manager:** The live toggle reads Auto preview changes; Constant shows as a disabled mode pill and the second tab reads Text Options; the properties tab lists a read-only Plot style row reporting the default.
