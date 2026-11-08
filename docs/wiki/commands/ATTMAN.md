---
summary: Opens the Block Attribute Manager, where you edit, reorder, remove and synchronize the attribute definitions of a block.
related: ATTDEF, ATTSYNC, ATTREDEF, ATTEDIT, BEDIT
ribbon: Insert tab › Block Definition panel; Draw tab › Block panel (slide-out)
---

## Usage
1. Run `ATTMAN`. The Block Attribute Manager opens on the block of a selected block reference, or on the first block that has attributes.
2. Choose the block from the list, or pick a block reference in the drawing with the select button. The dialog shows how many references of the block are in the drawing and in model space.
3. Select an attribute in the list.
4. Move it up or down, remove it, or click **Edit** to change it.
5. Click **Apply** or **OK** to write the changes to the block definition.

## Options
| Option | What it does |
|---|---|
| **Edit** | Opens the editor for the selected attribute, with pages for the attribute (tag, prompt, default and modes), the text (style, justification, height, rotation, width factor, oblique angle, boundary width, annotative, backwards, upside down) and the properties (layer, linetype, color, lineweight). |
| **Sync** | Updates the existing references of the block to the current attribute definitions. |
| **Columns** | Chooses which columns the attribute list shows. |
| **Apply changes to existing references** | When on, the changes are also applied to the block references already in the drawing. |
| **Emphasize duplicate tags** | Highlights attributes that share a tag. |
| **Show changes in the drawing as they are made** | Updates the drawing while you edit. |

## Notes
- `BATTMAN` runs the same command.
- If no block in the drawing has attributes, the command line says so and the dialog does not open.
- The last attribute of a block cannot be removed.
- The Constant mode cannot be changed here.
