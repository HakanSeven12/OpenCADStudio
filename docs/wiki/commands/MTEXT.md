---
summary: Creates a multiline text object in a text box, with paragraphs, columns and rich formatting.
related: TEXT, TEXTEDIT, STYLE, TXT2MTXT
ribbon: Annotate tab › Text panel; Draw tab › Annotation panel
---

## Usage
1. The command line shows the current text style, text height and whether the style is annotative.
2. Specify the first corner of the text box.
3. Specify the opposite corner, or type an option.
4. The text editor opens in the box. Type your text there.

## Options
At the opposite-corner prompt:

| Option | What it does |
|---|---|
| **Height** (`HEIGHT`) | Sets the text height. |
| **Justify** (`JUSTIFY`) | Where the text attaches to the box: TL, TC, TR, ML, MC, MR, BL, BC or BR. |
| **Line spacing** (`LINESPACING`) | Choose **At least** (`A`) or **Exactly** (`E`), then a spacing factor (0.25x to 4x, typed like `1.5x`) or a distance. |
| **Rotation** (`ROTATION`) | Sets the rotation angle of the text. |
| **Style** (`STYLE`) | Sets the text style. `?` lists the styles. |
| **Width** (`WIDTH`) | Type the box width instead of picking the opposite corner. The editor opens at once, and the box grows with the text. |
| **Columns** (`COLUMNS`) | **Dynamic** (`D`): column width, gutter and height, with columns added as text flows. **Static** (`S`): total width, number of columns, gutter and height. **No columns** (`N`) turns columns off. |

## Notes
- The default height comes from the current text style. With an annotative style, the height is scaled by the current annotation scale.
- After an option you return to the corner prompt, so you can set several options before you pick the box.
