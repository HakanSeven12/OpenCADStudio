---
summary: Removes everything from the current drawing and resets it to an empty new drawing.
related: NEW, ERASE, UNDO
---

## Usage
1. Run `CLEAR`. There are no prompts.
2. All objects, layers and settings of the drawing are replaced by those of an empty new drawing with the standard linetypes. The command line shows "Scene cleared. Standard linetypes loaded."

## Notes
- The tab is no longer tied to its file, so the next save asks for a file name. The original file on disk is not changed until you save over it.
- You can bring the drawing back with [`UNDO`](UNDO-Command).
- To remove only some objects, use [`ERASE`](ERASE-Command).
