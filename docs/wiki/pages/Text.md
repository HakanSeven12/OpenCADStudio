# Text

There are two kinds of text objects:

| | Kind | Command | Use it for |
|---|---|---|---|
| <img src="images/icons/text.svg" width="24" alt=""> | Single-line text | `TEXT` (`DT`) | Labels and short notes. Each line you type is a separate object. |
| <img src="images/icons/mtext.svg" width="24" alt=""> | Multiline text | `MTEXT` (`MT`, `T`) | Paragraphs, notes and anything with mixed formatting. One object holds all of it. |

## Single-line text

Start `TEXT`, click the start point (or choose **Justify** to set how the text is anchored), give the height and the rotation, then type. Enter starts a new line, which becomes a new text object; an empty line ends the command.

## Multiline text

Start `MTEXT` and drag a box for the text width. The text editor opens in place, showing the text as it will look in the drawing.

**Character formatting** (applies to the selected characters): <img src="images/icons/mt_bold.svg" width="20" alt=""> bold, <img src="images/icons/mt_italic.svg" width="20" alt=""> italic, <img src="images/icons/mt_underline.svg" width="20" alt=""> underline, <img src="images/icons/mt_overline.svg" width="20" alt=""> overline, <img src="images/icons/mt_strike.svg" width="20" alt=""> strikethrough, <img src="images/icons/mt_superscript.svg" width="20" alt=""> superscript, <img src="images/icons/mt_subscript.svg" width="20" alt=""> subscript, <img src="images/icons/mt_upper.svg" width="20" alt=""> UPPERCASE, <img src="images/icons/mt_lower.svg" width="20" alt=""> lowercase, font, height, colour, oblique angle, tracking and width factor.

**Paragraphs**: <img src="images/icons/mt_align_left.svg" width="20" alt=""> left, <img src="images/icons/mt_align_center.svg" width="20" alt=""> centre, <img src="images/icons/mt_align_right.svg" width="20" alt=""> right and <img src="images/icons/mt_align_justify.svg" width="20" alt=""> justified alignment; line spacing; first-line, left and right indents; space before and after; bullets and numbered lists.

**Columns**: no columns, static columns, or dynamic columns with automatic or manual height; set the count, width and gutter.

**Stacks**: select text such as `1/2` or `x^2` and choose **Stack** to turn it into a fraction or a tolerance stack.

**Symbols**: the Symbols menu inserts degree, plus/minus, diameter, squared, cubed, ≤, ≥, ≈, ≠, angle, delta, centreline, property line, ohm and others, and a non-breaking space. In any text you can also type the codes `%%d` (degree), `%%p` (plus/minus) and `%%c` (diameter).

**Find and replace** works inside the text being edited. **Clear formatting** removes the formatting of the selection and keeps the text.

Click outside the editor to finish.

## Editing text

Double-click a text to edit it in place (`TEXTEDIT`, `ED`). Other text tools:

| Command | What it does |
|---|---|
| `FIND` (**Ctrl+F**) | Finds and replaces text across the drawing: text, multiline text, attributes, dimensions, tables, hyperlinks. |
| `SPELL` (`SP`) | Checks the spelling of text in the drawing. |
| `SCALETEXT` | Changes the height of several texts at once, each about its own insertion point. |
| `JUSTIFYTEXT` | Changes the justification point without moving the text. |
| `TEXTALIGN` | Lines up several texts. |
| `TCASE` | Changes the case of text: upper, lower, sentence or title case. |
| `TORIENT` | Turns texts so that they read upright. |
| `TXT2MTXT` | Joins single-line texts into one multiline text. |
| `TEXTMASK` | Puts a mask behind text to hide what is underneath. |
| `ARCTEXT` | Places text along an arc. |
| `TEXTTOFRONT` | Brings all text and dimensions in front of other objects. |

## Text styles

<img src="images/icons/text_style.svg" width="24" alt=""> `STYLE` (`ST`) opens the text style manager. A style sets the font, height, width factor and oblique angle that texts use.

![The text style manager](images/screens/text-style.png)


- **Fonts** can be stroke fonts (SHX) or TrueType fonts. The font list shows where each font comes from: the drawing's folder, your font folders, or the system.
- A **height of 0** means the height is asked each time you place text; any other value fixes it.
- **Annotative** styles give text a paper height that is kept at every annotation scale; see [Annotation Scaling](Annotation-Scaling).
- **Backwards** and **Upside down** affect single-line text and attributes.
- Before you apply a change, the manager lists what it will change and how many texts use the style. Formatting set inside a multiline text is kept.
- **Import Styles** copies styles from another drawing.

**Missing fonts.** If a drawing uses a font that is not on your computer, the text is drawn with a substitute and the style manager marks the font as missing. Put the missing SHX or TrueType file in the drawing's folder or in a font folder (**Options › Files**) to use it.

## Fields

<img src="images/icons/data_link.svg" width="24" alt=""> `FIELD` inserts a field: text that updates itself. Fields can show drawing properties (name, author, date), object properties (an area, a length, a block attribute), sheet set data, or a formula. `UPDATEFIELD` refreshes them. With `FIELDDISPLAY` on, fields show with a grey background on screen so you can tell them from plain text.
