---
summary: Updates the block references of a block so their attributes match the block's current attribute definitions.
related: ATTMAN, ATTREDEF, ATTDEF, BEDIT
ribbon: Draw tab › Block panel (slide-out); Insert tab › Block Definition panel (slide-out)
---

## Usage
1. Enter an option [?/Name/Select]. Press Enter to select.
2. Select a block reference, then answer ATTSYNC block <name>? [Yes/No] <Yes>.
3. Or type **Name** (`N`) and enter the name of the block to sync.

The command line shows "ATTSYNC complete." when the references are updated.

You can also type the block name on the same line, for example `ATTSYNC TITLEBLOCK`.

## Options
| Option | What it does |
|---|---|
| **?** (`?`) | Lists the blocks in the drawing that have attributes. |
| **Name** (`N`) | Asks for the block name instead of a pick. |
| **Select** (`S`) | Asks you to select a block reference. |

## Notes
- Each attribute takes the position and text settings of its definition, placed through the reference's insertion point, scale and rotation.
- Attribute values are kept. New attributes get the default value. Attributes whose definition was removed are dropped.
- The references of a dynamic block are updated too.
- References on locked layers are skipped.
- If the block has no attributes, a message says so.
