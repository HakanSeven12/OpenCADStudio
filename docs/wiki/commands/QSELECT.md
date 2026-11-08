---
summary: Selects objects by type and property values, through the Quick Select panel or a one-line condition.
related: FILTER, SELECTSIMILAR, PROPERTIES
---

## Usage
1. Run the command. The **Quick Select** panel opens.
2. Under **Search in**, choose the current space or the current selection. **Window…** lets you pick the objects to search in.
3. Choose an **Object type**, a **Property**, an operator and a **Value**. Click **Add** to add the condition.
4. Add more conditions if needed. The panel shows how many objects match.
5. Choose the **Result**, then click **Select**.

## Options
| Option | What it does |
|---|---|
| **Search in** | Searches the current space or the current selection. |
| **Window…** | Steps aside so you can pick the objects to search in. |
| **Select object** | Pick an object to use its properties as conditions. Its layer, color and linetype are ticked; tick others, then **Add ticked conditions**. |
| Operators | **\* Any value**, **= Equals**, **!= Not equal**, **> Greater than**, **>= Greater or equal**, **< Less than**, **<= Less or equal**. The ordering operators are offered for numeric properties only. |
| **Conditions** | The conditions added so far. Each one can be edited or removed; **Clear** removes them all. |
| **Select the objects that do not match** | Turns the result around. |
| **Result** | **New selection** replaces the selection, **Add** adds to it, **Remove** takes the matches out of it. |
| **Show** | Highlights the matching objects in the drawing. |
| **Advanced…** | Opens the conditions in [`FILTER`](FILTER-Command), which has groups and NOT. |

## Command-line form
Type the condition after the command name:

`QSELECT [NOT] <property> [=|!=|<|<=|>|>=] <value>`

For example `QSELECT LAYER Walls`, `QSELECT TYPE Circle` or `QSELECT NOT LINETYPE Hidden`. Without an operator, `=` is used. Properties include `TYPE`, `LAYER`, `COLOR`, `LINETYPE`, `LINEWEIGHT`, `LTSCALE`, `PLOTSTYLE`, `TRANSPARENCY`, `MATERIAL`, `HYPERLINK` and `HANDLE`. The objects found in the current space become the selection. This form suits scripts.

## Notes
- Values accept wildcards (`*`, `?`, `#`, `~`) and comma-separated lists.
- Numbers compare as numbers, so `3` also finds `3.0000`.
- Conditions on different object types are combined so that an object matches when all conditions for its type match.
- Typed as `'QSELECT` while another command asks you to select objects, it finds the objects and hands them to that command.
