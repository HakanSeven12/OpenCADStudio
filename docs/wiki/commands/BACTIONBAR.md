---
summary: In the Block Editor, shows, hides or resets the action bars of dynamic-block parameters.
related: BACTION, BACTIONSET, BEDIT
---

## Usage
1. Choose **Show**, **Hide** or **Reset**.
2. For Show and Hide, select a parameter, enter its name, or type **All**. Press Enter to use the selected parameter, or all parameters when none is selected.

## Options
| Option | What it does |
|---|---|
| **Show** (`S`) | Shows the action bar of the chosen parameter. |
| **Hide** (`H`) | Hides the action bar of the chosen parameter. |
| **Reset** (`R`) | Shows all action bars again and puts them back at their default positions. |
| **All** (`A`) | Applies Show or Hide to every parameter. |

## Notes
- The command works only while a block is open in the Block Editor.
- Action bars must be turned on with [`BACTIONBARMODE`](BACTIONBARMODE-System-Variable) set to 1. Otherwise an error says so.
