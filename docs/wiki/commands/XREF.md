---
summary: Opens the External References palette to attach, reload, unload, detach and bind references.
related: XATTACH, EXTERNALREFERENCES, XRELOAD, XOPEN, XCLIP, REFEDIT
web: In the web app the reference list is read-only; reference changes are not available.
---

## Usage
1. Run `XREF`. The External References palette opens.
2. Select a reference in the list and use the toolbar or the right-click menu to work with it.

## Command-line form
`-XREF` works on the command line and suits scripts. It asks `Enter an option [?/Bind/Detach/Path/pathType/Unload/Reload/Overlay/Attach/Show] <Attach>:`.

| Option | What it does |
|---|---|
| **?** | Lists the references matching a name pattern (`*` lists all) with their type and path. |
| **Bind** (`B`) | Turns the named references into ordinary blocks of this drawing. |
| **Detach** (`D`) | Removes the named references. |
| **Path** (`P`) | Shows the old path of a reference and asks for a new one. |
| **pathType** (`T`) | Changes how the path is stored: **Full** (`F`), **Relative** (`R`) or **None** (`N`). |
| **Unload** (`U`) | Unloads the named references; they stay listed but are not shown. |
| **Reload** (`R`) | Reads the named references again from their files. |
| **Overlay** (`O`) | Asks for a file and attaches it as an overlay. |
| **Attach** (`A`) | Asks for a file and attaches it. Enter at the first prompt chooses this. |
| **Show** (`S`) | Opens the External References palette. |

Names accept the `*` and `?` wildcards; if nothing matches you see `No matching xref names found.`

## Notes
- The one-line form `XREF <option> <names>` works too, for example `XREF Reload *`, `XREF Path <name> <new path>` or `XREF Pathtype Relative <names>`. `XREF Path Find <old> <new>` replaces a part of the path in every reference. `XREF Overlay <names>` changes existing references to overlays.
- Relative paths are resolved from the drawing's folder, so save a new drawing before you reload its references.
