---
summary: Writes selected objects, a block definition or the whole drawing to a new drawing file.
related: BLOCK, -BLOCK, INSERT, XATTACH
ribbon: Insert tab › Block Definition panel (Create Block drop-down › Write Block)
web: In the web app the new drawing is downloaded by the browser.
---

## Usage
1. Optionally select objects first; they are taken into the dialog.
2. Run `WBLOCK`. The Write Block dialog opens.
3. Under **Source**, choose what to write.
4. For **Objects**, set the base point and the objects.
5. Under **Destination**, set the file name and path and the insert units.
6. Click **OK**.

## Dialog
| Setting | What it does |
|---|---|
| **Block** | Writes a block definition of the drawing, chosen from the list. |
| **Entire drawing** | Writes the whole drawing. |
| **Objects** | Writes the objects you select. |
| **Base point** | **Pick point** picks the base point in the drawing, or type **X**, **Y** and **Z**. Objects only. |
| **Select objects** / quick select button | Picks the objects to write, or opens quick select. The dialog shows how many objects are selected. |
| **Retain** / **Convert to block** / **Delete from drawing** | What happens to the selected objects in the current drawing after they are written. |
| **File name and path** | Where the new drawing is saved. The `...` button opens a file browser. It starts as `new block` in the drawing's folder, or in your documents folder for an unsaved drawing. |
| **Insert units** | The units stored in the new drawing. |

## Command-line form
`-WBLOCK` asks everything on the command line, which suits scripts:
1. `Enter name of output file:` (`.dwg` is added when you give no extension). If the file exists, you are asked whether to replace it.
2. `Enter name of existing block or [= (block=output file)/* (whole drawing)] <define new drawing>:` Type a block name, `=` for the block with the same name as the file, `*` for the whole drawing, or press Enter to write objects.
3. For objects: `Specify insertion base point or [mOde]:`, then `Select objects:` (skipped when objects were selected before). **mOde** (`O`) chooses **Convert to block** (`C`), **Retain** (`R`) or **Delete** (`D`).

When [`FILEDIA`](FILEDIA-System-Variable) is 1, `-WBLOCK` asks for the output file in a save dialog and the rest on the command line.

## Notes
- `WBLOCK <block name>` writes that block and `WBLOCK *` writes the selected objects; both ask for the file in a save dialog.
- The dialog refuses to write to the current drawing's own file.
- A failed write is reported on the command line.
- The command is not available inside the block editor.
