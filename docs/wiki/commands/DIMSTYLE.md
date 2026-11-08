---
summary: Opens the Dimension Style Manager, where you create, edit, compare and set the current dimension style.
related: DIMOVERRIDE, DIMLINEAR, STYLE, MLEADERSTYLE
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Run `DIMSTYLE`. The Dimension Style Manager opens.
2. Pick a style in the list on the left. A preview and the style settings show on the right.
3. Change settings on the tabs: **Lines**, **Symbols and Arrows**, **Text**, **Fit**, **Primary Units**, **Alternate Units** and **Tolerances**.
4. Click **Apply** to save the changes.

The buttons above the list:

| Button | What it does |
|---|---|
| **New** | Adds a new style. |
| **Copy** | Copies the selected style. |
| **Delete** | Deletes the selected style. |
| **Import** | Brings dimension styles in from another drawing. If a name already exists, you choose to overwrite or skip it. |
| **Set Current** | Makes the selected style the one new dimensions use. |
| **Apply** | Saves the edits to the selected style. |

## Notes
- Double-click a style name to rename it.
- **Compare with** lists the tab sections in which the selected style differs from another style.
- Styles from external references are shown but cannot be edited.
- The corner arrow of the Annotate tab's Dimensions panel opens this dialog. The style list in the same panel sets the current style.
- The **Update** button in the Dimensions panel runs `-DIMSTYLE` with **Apply**.

## Command-line form
`-DIMSTYLE` works without the dialog, so it suits scripts. It first shows the current style and whether it is annotative, then asks for an option:

| Option | What it does |
|---|---|
| **ANnotative** (`AN`) | Asks whether the new style is annotative, then for a name. Saves the current settings under that name and makes it current. The options are asked again afterwards. |
| **Save** (`S`) | Asks for a name and saves the current style's settings under it. If the name is in use, asks whether to redefine it. The saved style becomes current. |
| **Restore** (`R`) | Asks for a style name and makes it current. Press Enter instead to select a dimension and use its style. This is the default. |
| **STatus** (`ST`) | Lists the values of the current style. |
| **Variables** (`V`) | Asks for a style name and lists its values. |
| **Apply** (`A`) | Asks you to select objects. The selected dimensions take the current style, and their overrides are removed. |
| **?** | Asks which styles to list (`*` for all) and lists their names. |

Type `?` at a name prompt to list the styles.
