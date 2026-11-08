---
summary: Opens the External References panel, which lists and manages the drawings, images and underlays attached to the drawing.
related: EXTERNALREFERENCESCLOSE, XATTACH, XREF, IMAGEATTACH, PDFATTACH
ribbon: Insert tab › Reference panel
web: The panel is read-only in the web app: you cannot attach or refresh files from it.
---

## Usage
1. Type `EXTERNALREFERENCES`, or click the corner launcher of the Reference panel.
2. The External References panel opens docked on the right. If it is already open, it stays open.

## In the panel
- The toolbar's **Attach DWG** button (with a menu for **Attach Image**, **Embed Image (in drawing)** and **Attach PDF**) and **Refresh** button (with **Reload All References**) attach and reload files.
- **File References** lists every reference with its status (Loaded, Unloaded, Not found, Unresolved, and so on), size, type, date and saved path. Switch between **List** and **Tree** view.
- Right-click a reference to **Open**, **Attach...**, **Unload**, **Reload** or **Detach** it, or change its path (**Select New Path**, **Find and Replace...**, **Make Absolute**, **Make Relative**, **Remove Path**).
- The **Details** and **Preview** area shows the selected reference's properties.

## Notes
- Nested references are read-only here. Edit them in their host drawing.
- Relative paths need the drawing to be saved first.
- The panel also opens from the contextual tabs of PDF underlays and point clouds.
- Close the panel with [`EXTERNALREFERENCESCLOSE`](EXTERNALREFERENCESCLOSE-Command).
