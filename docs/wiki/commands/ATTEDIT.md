---
summary: Opens the attribute editor for a block reference, where you change its attribute values, text options and properties.
related: ATTDEF, EATTEDIT, ATTMAN, ATTSYNC, ATTDISP
ribbon: Insert tab › Block panel (Edit Attribute drop-down); Draw tab › Block panel
---

## Usage
1. Select block with attributes, or select one block reference before you start the command.
2. The attribute editor opens. Pick an attribute in the list and change its value.
3. Use the **Text Options** and **Properties** pages to change the text style, height, justification, rotation, width factor, oblique angle, layer, color, linetype and lineweight of the attribute.
4. Click **Apply** to keep the changes.

## Command-line form
`-ATTEDIT` edits attributes on the command line, so it suits scripts. On the ribbon it is the **Multiple** item of the Edit Attribute drop-down.
1. Edit attributes one at a time? [Yes/No] <Y>:
2. For global editing (No), answer Edit only attributes visible on screen? [Yes/No] <Y>:
3. Enter block name specification, attribute tag specification and attribute value specification. Press Enter for `*` (all). Wildcards are allowed.
4. For global editing, the number of matching attributes is shown. Enter string to change, then Enter new string. The text is replaced inside every matching value.
5. For one-at-a-time editing, the attribute editor opens after the questions.

You can also give the work on one line:
- `-ATTEDIT REPLACE <old string> <new string> [block] [tag] [value]` replaces text inside matching values.
- `-ATTEDIT RENAME <old tag> <new tag>` renames a tag on the selected block references, or on every block reference when nothing is selected.
- `-ATTEDIT <tag> <value>` sets that tag to the value on the selected block references. With only a tag, or nothing, it lists the current values.

## Notes
- If the block has no attributes, or you pick something that is not a block reference, a message says so.
- Block references on locked layers cannot be edited.
- Changes can be undone.
