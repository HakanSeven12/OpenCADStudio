---
summary: Opens the Files palette, which lists a folder's sub-folders and drawings to open a drawing with a double-click or drag it onto the drawing area as a block.
related: FILESPALETTECLOSE, OPEN, BLOCKSPALETTE, INSERT
ribbon: View tab › Palettes panel › Files
web: Not available in the web app.
---

## Usage
Run `FILESPALETTE`, or click **Files** on the View tab. `FILESPALETTECLOSE` closes the palette.

The palette shows one folder: its sub-folders first, then its drawings (DWG and DXF), each sorted by name. Hidden files and folders (a leading dot) are left out.

| Control | What it does |
|---|---|
| Up | Shows the folder above. |
| Folder button | Chooses the folder to show. |
| Refresh | Reads the folder again. |
| Filter | Shows only the entries whose name contains the text typed (case does not matter). |
| Double-click a drawing | Opens it in its own tab, as dropping it on the window does. A tab already showing it comes to the front. |
| Double-click a folder | Shows that folder. |
| Drag a drawing onto the drawing area | Inserts it there as a block, as a library drawing from the Blocks palette is. |
| Click a drawing | Selects it; its preview shows under the list. |

The context menu of a drawing offers **Open Drawing**, **Insert** (place it as a block) and **Open File Location**; that of a folder **Open** and **Open File Location**.

## Notes
- The palette remembers the folder it showed and whether it was open, across sessions.
- The preview is the one saved in the drawing; a drawing without one shows its model space.
