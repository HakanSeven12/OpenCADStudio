---
summary: Creates single-line text; each line you type becomes a separate text object.
related: MTEXT, TEXTEDIT, STYLE, JUSTIFYTEXT, TEXTALIGN
ribbon: Draw tab › Annotation panel; Annotate tab › Text panel
---

## Usage
1. The command line shows the current text style, height, annotative setting and justification.
2. Specify the start point of the text, or type an option.
3. Specify the height (type a value or pick a point). This step is skipped when the text style has a fixed height.
4. Specify the rotation angle (type a value or pick a point).
5. Type the text in the in-place editor. Press Enter to start the next line below; each line is its own object.
6. Press Enter on an empty line, or Esc, to finish.

## Options
| Option | What it does |
|---|---|
| **Justify** (`J`) | Choose how the text is placed on its point: **Left**, **Center**, **Right**, **Align**, **Middle**, **Fit**, or one of **TL**, **TC**, **TR**, **ML**, **MC**, **MR**, **BL**, **BC**, **BR**. The point prompt then names the point to pick (for example "Specify top-left point of text"). |
| **Style** (`S`) | Enter a text style name for the new text. Type `?` to list the styles in the drawing. |

## Notes
- **Align**: pick both ends of the baseline; the height follows from the length.
- **Fit**: pick both ends of the baseline, then the height; the text is squeezed or stretched to fit between the points.
- Pressing Enter at the first prompt (without picking a point) continues one line below the last line you wrote in this drawing.
- The rotation you type is remembered for the next `TEXT`.
- With an annotative text style, the height prompt asks for the paper text height.
