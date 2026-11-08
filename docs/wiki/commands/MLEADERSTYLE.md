---
summary: Opens the Multileader Style manager, where you create, edit and set the current multileader style.
related: MLEADER, MLSTYLE, DIMSTYLE, TABLESTYLE
ribbon: Annotate tab › Leaders panel
---

## Usage
1. Type `MLEADERSTYLE`, or click the arrow at the corner of the Leaders panel. The style manager opens.
2. Pick a style in the list to see its preview and details.
3. Use the buttons to create a new style, copy, rename or delete a style, set it current, or apply it to selected multileaders.
4. Edit the selected style on its four tabs: **Leader Format** (leader line and arrowhead), **Leader Structure** (leader points, segment angles, landing and scale), **Content** (text settings) and **Block Content** (block settings).

## Options
You can also type a sub-option after the command name, which suits scripts:

| Option | What it does |
|---|---|
| **LIST** (`?`) | Lists the styles with text height, arrow size, landing distance and how many multileaders use each. |
| **NEW** `<name> [property value ...]` | Creates a style, optionally setting properties in pairs, e.g. `NEW NOTE TEXT 2.5`. |
| **SET** `<name> <property> <value>` | Changes one property of a style. |
| **APPLY** `<name> [ALL]` | Gives the style to the selected multileaders, or to every multileader with `ALL`. |
| **INFO** `[ALL]` | Prints which style each selected (or every) multileader uses. |
| **CURRENT** `[<name>]` | Shows the current style, or makes the named style current. |

Properties for NEW and SET: `text_height`, `arrowhead_size`, `landing_distance`, `landing_gap`, `scale` and `arrowhead` (an arrowhead name, or `.` for closed filled).

## Notes
- New multileaders use the current style. You can also choose it in the style list on the Leaders panel.
- The compare list shows how the selected style differs from another one.
