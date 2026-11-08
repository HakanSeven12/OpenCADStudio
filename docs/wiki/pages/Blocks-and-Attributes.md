# Blocks and Attributes

A **block** is a named group of objects stored once in the drawing and placed as many times as you need. Each placement is a **block reference**: it has its own position, scale and rotation, but all references share one definition. Change the definition and every reference changes.

Use blocks for anything that repeats: doors, windows, furniture, symbols, title blocks, standard details.

## Creating blocks

| | Tool | Command | What it does |
|---|---|---|---|
| <img src="images/icons/blocks/block.svg" width="24" alt=""> | Create | `BLOCK` (`B`) | Makes a block from selected objects in this drawing. Give a name, a base point (the point you place it by) and choose whether the objects are kept, turned into a reference, or deleted. |
| | Write Block | `WBLOCK` (`W`) | Saves objects, a block or the whole drawing as a separate drawing file. |
| <img src="images/icons/base_point.svg" width="24" alt=""> | Base | `BASE` | Sets the point by which the whole drawing is placed when it is inserted into another one. |

Options of the block definition: **Annotative**, **Scale uniformly** (the block cannot be stretched in one direction), **Allow exploding**, and the insertion units.

Objects drawn on layer 0 with colour, linetype and lineweight ByBlock take the properties of each reference. Objects on other layers keep their own.

## Inserting blocks

<img src="images/icons/blocks/insert.svg" width="24" alt=""> `INSERT` (`I`) opens the **Blocks** palette: the blocks of the current drawing, recent blocks, and blocks from other drawings or folders, each with a preview. Drag a block into the drawing, or click it and place it. Set the scale, rotation and whether it is exploded on insert.

`-INSERT` asks the same at the command line. `MINSERT` places a block as a rectangular array in a single reference.

A whole drawing file can be inserted as a block too; its model space becomes the block definition.

## Changing blocks

| | Tool | Command | What it does |
|---|---|---|---|
| <img src="images/icons/edit_block.svg" width="24" alt=""> | Block Editor | `BEDIT` (`BE`) | Opens the definition in the Block Editor; see below. |
| | Edit in place | `REFEDIT` | Edits a reference where it sits in the drawing; `REFCLOSE` saves or discards. |
| | Explode | `EXPLODE` (`X`) | Turns a reference back into separate objects. |
| | Redefine | `BLOCK` with an existing name | Replaces the definition; all references update. |
| | Rename | `RENAME` | Renames blocks and other named items. |
| <img src="images/icons/purge.svg" width="24" alt=""> | Purge | `PURGE` (`PU`) | Removes block definitions that are not used. |

`COUNT` counts the references of a block, and the **Count** palette (`COUNTLIST`) lists every block with its number of references.

## Attributes

An **attribute** is a labelled value stored in a block reference: a door number, a part code, the drawing title in a title block. The value can be different in every reference.

| | Tool | Command | What it does |
|---|---|---|---|
| <img src="images/icons/attdef.svg" width="24" alt=""> | Define Attributes | `ATTDEF` (`ATT`) | Creates an attribute definition: a tag, a prompt, a default value and text settings. Include it when you create the block. |
| <img src="images/icons/attedit.svg" width="24" alt=""> | Edit Attributes | `ATTEDIT` (`ATE`) | Edits the values of a reference. Double-clicking a reference does the same. |
| <img src="images/icons/attman.svg" width="24" alt=""> | Attribute Manager | `ATTMAN` | Edits, reorders and removes the attribute definitions of a block. |
| <img src="images/icons/attsync.svg" width="24" alt=""> | Synchronize | `ATTSYNC` | Updates existing references after the block's attribute definitions changed. |
| | Redefine | `ATTREDEF` | Redefines a block from the objects you select, keeping the attribute values of existing references. |
| | Display | `ATTDISP` | Shows attributes as set, all on, or all off. |

Attribute modes: **Invisible**, **Constant** (same in every reference), **Verify**, **Preset** (uses the default without asking), **Lock position** and **Multiple lines**.

When you insert a block with attributes, its prompts ask for the values, either at the command line or in a dialog (`ATTDIA`).

`DATAEXTRACTION` collects attribute values from many references into a table or a file; see [Leaders and Tables](Leaders-and-Tables#table-styles-and-data).

## The Block Editor

`BEDIT` opens a block definition in its own editing space with the **Block Editor** tab and the authoring palettes. Draw and edit as usual, then <img src="images/icons/mt_ok.svg" width="20" alt=""> **Close Block Editor** (`BCLOSE`) and save. `BSAVE` and `BSAVEAS` save without closing.

### Dynamic blocks

Dynamic blocks change shape through grips on each reference: a door that sets its width, a table that adds chairs, a symbol with several looks. You build them in the Block Editor by adding **parameters** and **actions**.

**Parameters** add grips and properties:

| Parameter | Gives |
|---|---|
| Point | A position. |
| Linear | A distance along a line. |
| Polar | A distance and an angle. |
| XY | Horizontal and vertical distances. |
| Rotation | An angle. |
| Alignment | Turns the block to line up with objects it is placed against. |
| Flip | Mirrors the block. |
| Visibility | Named states that each show different objects. |
| Lookup | A list of named values that sets other parameters together. |
| Base Point | A new base point for the block. |

**Actions** tie a parameter to the objects it changes: **Move**, **Scale**, **Stretch**, **Polar Stretch**, **Rotate**, **Flip**, **Array** and **Lookup**. A parameter can also offer a **value set**: fixed values or steps that its grip snaps to.

**Visibility states.** `BVSTATE` creates and switches visibility states; <img src="images/icons/constrain/show.svg" width="20" alt=""> `BVSHOW` and <img src="images/icons/constrain/hide_all.svg" width="20" alt=""> `BVHIDE` choose which objects show in the current state. Each reference then has a list to pick its state.

**Block properties table.** `BTABLE` defines combinations of parameter values in a table, so each reference picks a row.

`BTESTBLOCK` opens a test window to try the block's grips before you save it. `BCONSTRUCTION` marks objects as construction geometry that does not show in the references. Geometric and dimensional constraints work inside blocks too; see [Parametric Constraints](Parametric-Constraints).
