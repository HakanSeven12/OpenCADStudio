---
summary: In the Block Editor, links an action that has lost its parameter to a compatible parameter.
related: BACTION, BPARAMETER, BACTIONSET, BACTIONBAR
ribbon: Block Editor tab › Manage panel
---

## Usage
1. Select the marker of an unassociated action, or type the action's name. If such an action is already selected, this step is skipped.
2. Select a key point of a compatible parameter, or type the parameter's name.
3. If you typed the name of a non-point parameter for a Move, Stretch or Polar Stretch action, choose the key point the action follows. Press Enter for **End**.

## Options
| Option | What it does |
|---|---|
| **Base** (`B`) | The action follows the parameter's base point. |
| **X** (`X`) | The action follows the X key point. Only for XY parameters. |
| **Y** (`Y`) | The action follows the Y key point. Only for XY parameters. |
| **End** (`E`) | The action follows the parameter's end point. This is the default. |

## Notes
- The command works only in the Block Editor.
- If the block has no unassociated actions, the command reports it and ends.
- If no parameter fits any unassociated action, you are asked to add a compatible parameter first.
- Only parameters that suit the chosen action are offered.
- When actions are shown in action bars ([`BACTIONBARMODE`](BACTIONBARMODE-System-Variable) is 1), the command is refused. Edit the action from its action bar, or set [`BACTIONBARMODE`](BACTIONBARMODE-System-Variable) to 0.
