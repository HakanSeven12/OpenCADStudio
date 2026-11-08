---
summary: Opens a block definition in the Block Editor so you can change its geometry and add dynamic parameters and actions.
related: BLOCK, BCLOSE, BSAVE, BSAVEAS, BTESTBLOCK, BPARAMETER, BACTION, BAUTHORPALETTE
ribbon: Insert tab › Block Definition panel; Draw tab › Block panel; Block Editor tab › Open and Save panel
---

## Usage
1. If exactly one block reference is selected, the Block Editor opens on its block right away.
2. Otherwise the **Edit Block Definition** dialog opens. Pick a block from the list, or type a new name in **Block to create or edit**. The dialog shows a preview and the description of the chosen block.
3. Click **OK**. The block opens in the Block Editor as its own space, and the Block Editor ribbon tab appears.
4. Edit the block with the normal drawing and editing commands. Add parameters, actions and constraints from the Block Editor tab or the Authoring Palettes.
5. Save with [`BSAVE`](BSAVE-Command) or [`BSAVEAS`](BSAVEAS-Command), and leave with [`BCLOSE`](BCLOSE-Command).

## Command-line form
`-BEDIT` asks at the command line instead of opening the dialog:
1. Enter the block name, or `?` to list blocks.
2. With `?`, enter the block names to list (wildcards `*` and `?` work, separate several with commas). Press Enter to list all.

An existing name opens that block. A new name creates an empty block and opens it. This form suits scripts.

## Notes
- A new block that you close without saving is removed again.
- If another block is already open in the Block Editor, opening a different one first asks whether to save the changes.
- The command is refused while a reference edit is active; finish it first.
- The command is refused in a Test Block window.
- When [`BLOCKEDITLOCK`](BLOCKEDITLOCK-System-Variable) is 1, the Block Editor cannot be opened.
- The dialog lists only named blocks of this drawing. External references, blocks from them, layouts and anonymous blocks are not listed.
