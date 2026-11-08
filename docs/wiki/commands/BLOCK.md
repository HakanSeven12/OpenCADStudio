---
summary: Creates a block definition from selected objects in the current drawing.
related: BEDIT, INSERT, WBLOCK, BASE, ATTDEF, BLOCKPALETTE
ribbon: Insert tab › Block Definition panel; Draw tab › Block panel
---

## Usage
1. Run the command. The **Block Definition** dialog opens. Objects you selected before are already taken as the block's objects.
2. Enter the block **Name**.
3. Set the **Base point**: type X, Y and Z, or click **Pick point** and pick it in the drawing. With **Specify On-screen** on, you pick it after you click OK.
4. Choose the **Objects**: click **Select objects** to select them in the drawing. With **Specify On-screen** on, you select them after you click OK.
5. Choose what happens to the selected objects: **Retain**, **Convert to block** or **Delete**.
6. Set the behavior and settings you need, then click **OK**.

## Options
| Option | What it does |
|---|---|
| **Retain** | Keeps the selected objects as they are. |
| **Convert to block** | Replaces the selected objects with a reference of the new block. |
| **Delete** | Removes the selected objects from the drawing. |
| **Annotative** | Makes the block annotative. **Match block orientation to layout** then becomes available. |
| **Scale uniformly** | References of the block can only be scaled by the same factor in X, Y and Z. |
| **Allow exploding** | Allows references of the block to be exploded. |
| **Block unit** | The unit of the block, used to scale it when it is inserted. |
| **Hyperlink** | Attaches a hyperlink to the block. |
| **Description** | A description shown in block lists. |
| **Open in block editor after creating** | Opens the new block in the Block Editor right away. |

## Command-line form
`-BLOCK` works at the command line without the dialog. It first prints the current object conversion setting.
1. Enter the block name, or `?` to list the blocks of the drawing.
2. If the name already exists, answer whether to redefine it. Press Enter for **No**.
3. Specify the insertion base point, or type an option:
   - **Annotative** (`A`): asks whether to create an annotative block, then whether to orient it to the layout in paper space viewports.
   - **mOde** (`O`): asks what to do with the objects: **Convert to block** (`C`), **Retain** (`R`) or **Delete** (`D`).
4. Select the objects and press Enter. If objects were selected before the command, they are used and this step is skipped. An empty selection creates an empty block.

This form suits scripts.

## Notes
- If you click OK with no objects, the dialog offers **Select objects** or **Create empty block**.
- If a block with the same name exists, you are asked whether to redefine it. Redefining updates all its references.
- A block name cannot be empty, cannot start with `*`, cannot contain invalid characters and can be at most 255 characters long.
- The block's insertion unit defaults to the drawing's [`INSUNITS`](INSUNITS-System-Variable).
