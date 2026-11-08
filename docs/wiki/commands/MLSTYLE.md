---
summary: Opens the Multiline Style manager, where you create, edit and set the current multiline style.
related: MLINE, MLEDIT, MLEADERSTYLE
---

## Usage
1. Type `MLSTYLE`. The style manager opens.
2. Pick a style in the list to see its preview and details.
3. Use the buttons to create a new style, copy, rename or delete a style, set it current, or apply it to selected multilines.
4. Edit the selected style on its two tabs: **Elements** (add or delete lines, and set each line's offset, color and linetype) and **Caps and Fill** (start and end caps as line, outer arc or inner arcs with their angles, display joints, and fill color).

## Options
You can also type a sub-option after the command name, which suits scripts:

| Option | What it does |
|---|---|
| **LIST** (`?`) | Lists the styles with their number of elements, marking the current one. |
| **NEW** `<name>` (`N`) | Creates a style from the Standard style. |
| **SET** `<name>` (`S`) | Makes the named style current. |
| **DEL** `<name>` | Deletes a style. The Standard style cannot be deleted. |

## Notes
- New multilines drawn with [`MLINE`](MLINE-Command) use the current style.
- The compare list shows how the selected style differs from another one.
