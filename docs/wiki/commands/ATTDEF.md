---
summary: Creates an attribute definition: a tag, prompt and default value that becomes a fill-in text field when the definition is part of a block.
related: BLOCK, ATTEDIT, ATTMAN, ATTSYNC, ATTDISP, INSERT
ribbon: Insert tab › Block Definition panel; Draw tab › Block panel (slide-out); Block Editor tab › Action Parameters panel
---

## Usage
1. Run `ATTDEF`. The Attribute Definition dialog opens.
2. Under **Mode**, turn on the modes you need.
3. Under **Attribute**, enter the **Tag**, the **Prompt** and the **Default** value.
4. Under **Text Settings**, set the justification, text style, text height, rotation and, for multiple lines, the boundary width.
5. Under **Insertion Point**, turn on **Specify on-screen** or type the X, Y and Z coordinates.
6. Click **Define**. With **Specify on-screen** on, pick the start point in the drawing.

## Options
| Option | What it does |
|---|---|
| **Invisible** | The attribute value is not shown when the block is inserted. |
| **Constant** | The value is fixed. You are not asked for it when you insert the block. |
| **Verify** | You are asked to confirm the value when you insert the block. |
| **Preset** | The default value is used without asking when you insert the block. |
| **Lock position** | Locks the position of the attribute inside the block. |
| **Annotative** | The attribute text is annotative. |
| **Multiple lines** | The value can have several lines, inside a boundary width. |
| **Align below previous attribute definition** | Places the new definition right under the one you placed last in this drawing, with the same text settings. |
| **Insert field** | Puts a field into the default value. |
| **Multiline editor** | Opens a text editor for the default value of a multiple-line attribute. |

The text height, rotation and boundary width can also be picked in the drawing.

## Command-line form
`-ATTDEF` asks for everything on the command line, so it suits scripts:
1. It lists the current attribute modes. Enter an option to change [Invisible/Constant/Verify/Preset/Lock position/Annotative/Multiple lines], or press Enter when done. Each keyword switches that mode.
2. Enter attribute tag name. The tag cannot be empty.
3. Enter attribute prompt.
4. Enter default attribute value (or the attribute value for a constant attribute). For multiple lines, type one line at a time and press Enter on an empty line when done.
5. Specify start point of text, or type **Justify** (`J`) or **Style** (`S`). Press Enter to place it below the previous definition.
6. Specify the height and the rotation angle.

For a multiple-line attribute you specify the location and the opposite corner of its box instead. At the corner prompt you can type **Height** (`H`), **Justify** (`J`), **Line spacing** (`L`), **Rotation** (`R`), **Style** (`S`) or **Width** (`W`).

## Notes
- The tag may not contain spaces.
- The modes and the Annotative setting are kept for the next attribute definition. Multiple lines is turned off again each time.
- Use [`BLOCK`](BLOCK-Command) to put attribute definitions into a block. Double-click an attribute definition to edit its tag, prompt and default value.
