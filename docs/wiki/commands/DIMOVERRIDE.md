---
summary: Overrides dimension style settings on selected dimensions without changing the style, or clears those overrides.
related: DIMSTYLE, DIMEDIT, DIMTEDIT
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Type the name of a dimension variable to override, for example `DIMTXT`.
2. Type the new value, or press Enter to keep the value shown.
3. Repeat steps 1 and 2 for more variables. Press Enter at the variable name prompt when you are done.
4. Select the dimensions and press Enter. If dimensions were already selected, they are used directly.

## Options
| Option | What it does |
|---|---|
| **Clear overrides** (`C`) | Offered at the first prompt. Removes all overrides from the dimensions you then select. |

## Notes
- The dimension style itself does not change; only the selected dimensions get the new values.
- For arrowhead variables such as `DIMBLK`, type a block name; for `DIMTXSTY`, type a text style name.
- Values are checked: an unknown variable name, a text style or block that does not exist, or a value that does not fit the variable is rejected.
- Dimensions on locked layers are not changed.
