---
summary: Reloads all external references of the current drawing from their files.
related: XREF, XATTACH, EXTERNALREFERENCES
---

## Usage
1. Run `XRELOAD`. Every reference, including unloaded ones, is read again, and the command line reports the result for each.

## Notes
- Relative paths are resolved from the drawing's folder. In the desktop app, save a new drawing first; otherwise you see `XREF  Save the drawing first to resolve relative XREF paths.`
- The reload can be undone.
- To reload only some references, use `-XREF` with the **Reload** option or the External References palette.
