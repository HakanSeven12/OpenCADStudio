---
summary: In the Block Editor, changes which objects an existing action of a dynamic block acts on.
related: BACTION, BASSOCIATE, BACTIONBAR, BEDIT
ribbon: Block Editor tab › Manage panel
---

## Usage
1. Pick an action marker or enter its name.
2. Enter an option. Press Enter for **New**.
3. Select objects, then press Enter.

## Options
| Option | What it does |
|---|---|
| **New** | Replaces the selection of the action with the objects you select. |
| **Add** | Adds the objects you select to the action. |
| **Remove** | Removes the objects you select from the action. |
| **Frame** | For a stretch action, draws a new stretch frame: two corners, or **CPolygon** for a polygon frame. |
| **RotateOnly** | For a polar stretch action, selects the objects of the action that should only rotate. |

## Notes
- The command works only in the Block Editor, in a block that has actions.
- When action bars are on ([`BACTIONBARMODE`](BACTIONBARMODE-System-Variable) is 1), edit the action from its action bar instead, or set the variable to 0.
- On the ribbon the tool is labelled **Action Selection**.
