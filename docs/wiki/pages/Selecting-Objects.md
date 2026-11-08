# Selecting Objects

Most editing commands work on a set of selected objects. You can select first and then start the command, or start the command and select when it asks `Select objects`. Press Enter to finish selecting.

## Picking and windows

| Action | Selects |
|---|---|
| Click an object | That object. Click more to add them. |
| **Shift**+click a selected object | Removes it from the selection. |
| Click empty space, move **right**, click | A **window**: objects entirely inside the rectangle. |
| Click empty space, move **left**, click | A **crossing** window: objects inside it or crossing its edge. |
| Press and drag in empty space | A **lasso** around the objects. Dragging right or left gives window or crossing behaviour, as with the rectangle. |
| **Ctrl+A** | Everything in the current space, except objects on layers that are off or frozen. |
| **Esc** | Clears the selection. |

Set `PICKDRAG` to make a press-and-drag draw a rectangle instead of a lasso. With `PICKADD` set to 0 (**Options › Selection › Use Shift to add to selection**), a click replaces the selection instead of adding to it; Shift then adds.

Objects on locked layers can be seen and snapped to, but not selected.

## Selection methods at the prompt

When a command asks you to select objects, you can also type one of these:

| Type | Method |
|---|---|
| `W` | Window |
| `C` | Crossing |
| `BOX` | Window or crossing, depending on the direction |
| `ALL` | Every object |
| `L` | The last object drawn |
| `P` | The previous selection |
| `F` | Fence: objects crossed by a polyline you draw |
| `WP` | Window polygon: objects entirely inside a polygon |
| `CP` | Crossing polygon: objects inside or crossing a polygon |
| `G` | All objects of a named group |
| `R` / `A` | Remove mode / back to add mode |
| `U` | Undo the last selection step |
| `M` | Multiple picks before the objects are highlighted |
| `SI` | Single: the first pick or window ends the selection |
| `AU` | Auto: a pick on an object selects it, a pick in empty space starts a window |
| `SU` | Subobject: faces, edges and vertices of solids |

## Overlapping objects

<img src="images/icons/status/selcycle.svg" width="20" alt=""> When objects lie on top of each other, turn on **Selection Cycling** (**Ctrl+W**). A click then lists the objects under the cursor, and you choose the one you want.

## Selecting by property

<img src="images/icons/blocks/quick_select.svg" width="20" alt=""> **Quick Select** (`QSELECT`) selects every object that matches a condition: all circles on layer `HOLES`, every text with a height of 2.5, all blocks named `DOOR`. Pick the object type, a property, an operator and a value; the result count updates while you choose. You can add to the current selection or select from inside it.

![Quick Select](images/screens/quick-select.png)

<img src="images/icons/status/filter.svg" width="20" alt=""> **Filter** (`FILTER`, `FI`) builds more complex conditions: several rules joined with AND and OR, grouped, saved under a name and reused. `FILTER <name>` applies a saved filter directly, which is handy in scripts.

`SELECTSIMILAR` selects all objects of the same type and the same properties as the ones you pick; `SELECTSIMILAR` › **Settings** chooses which properties have to match.

`SELHANDLES` selects objects by their handles, as a list typed at the command line.

## Groups

<img src="images/icons/group.svg" width="20" alt=""> `GROUP` (`G`) joins objects into a named group: picking one member selects them all. <img src="images/icons/ungroup.svg" width="20" alt=""> `UNGROUP` (`UG`) dissolves a group. Groups are saved in the drawing.

## Isolating and hiding

<img src="images/icons/status/isolate.svg" width="20" alt=""> `ISOLATEOBJECTS` hides everything except the selected objects; `HIDEOBJECTS` hides the selected ones; `UNISOLATEOBJECTS` shows everything again. Hidden objects are only hidden on screen: they are still in the drawing and still plot. To hide whole layers, see [Layers and Object Properties](Layers-and-Properties#layer-tools).
