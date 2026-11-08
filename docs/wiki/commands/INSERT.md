---
summary: Inserts a block into the drawing from the Blocks palette.
related: BLOCKSPALETTE, BLOCK, WBLOCK, MINSERT, EXPLODE, BEDIT
ribbon: Insert tab › Block panel
web: Inserting a drawing file by typing its name at the -INSERT prompt is not available in the web app.
---
## Usage
1. Run `INSERT`. The Blocks palette opens on its **Current Drawing** tab.
2. Click a block in the palette. The palette's options decide which values are asked.
3. Specify the insertion point, or type an option.
4. Answer the remaining prompts for scale and rotation, if any.
5. If the block has attributes, enter a value for each one, or press Enter to keep the default.

## Command-line form
`-INSERT` runs entirely on the command line and suits scripts.

1. `Enter block name or [?] <last>:` type a block name, or pick it from the list shown as you type. Type `?` to list the blocks. If no block has that name, the name is looked up as a DWG file and the drawing is brought in as a block.
2. `Specify insertion point or [Basepoint/Scale/X/Y/Z/Rotate/Explode/REpeat]:`
3. `Enter X scale factor, specify opposite corner, or [Corner/XYZ] <1>:`
4. `Enter Y scale factor <use X scale factor>:`
5. `Specify rotation angle <0>:`
6. Enter the attribute values, if the block has attributes.

| Option | What it does |
|---|---|
| **Basepoint** (`B`) | Picks a different base point on the block for this insertion. |
| **Scale** (`S`) | Sets one scale factor for X, Y and Z before you place the block. |
| **X** / **Y** / **Z** (`X`, `Y`, `Z`) | Sets the scale factor for one axis before you place the block. The others stay at 1. |
| **Rotate** (`R`) | Sets the rotation angle before you place the block. |
| **Explode** (`E`) | Inserts the block as separate objects. |
| **REpeat** (`RE`) | Keeps inserting the same block; you are asked whether each one gets its own scale and rotation. |
| **Corner** (`C`) | Sets the X and Y scale from an opposite corner point. |
| **XYZ** | Asks for separate X, Y and Z scale factors. |

## Notes
- When a block has more than one possible insertion point, the prompt says so and Ctrl cycles through them.
- Scale factors must be nonzero.
