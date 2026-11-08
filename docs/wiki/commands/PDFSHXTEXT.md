---
summary: Recognises text that was drawn as SHX font strokes in imported PDF geometry and replaces it with multiline text.
related: PDFIMPORT, TXT2MTXT
ribbon: Insert tab › Import panel
---

## Usage
1. Start `PDFSHXTEXT`.
2. Select the geometry to convert, or type **SEttings** (`SE`) to change the recognition settings first.
3. Press Enter to finish the selection.

The command line reports each group of text it found, whether it was converted and with which font, and how many text objects were created.

## Options
| Option | What it does |
|---|---|
| **SEttings** (`SE`) | Opens the PDF Text Recognition Settings. |

The settings choose:

| Setting | What it does |
|---|---|
| **SHX fonts to compare** | The fonts tried, in order from the top. You can turn fonts on or off, reorder them, and add your own SHX font files. By default simplex, txt and isocp are on. |
| **Recognition threshold** | The share of a group (1 to 100 percent) that must be recognised for it to be converted. The default is 95. |
| **Use best matching font** | Compares every font that is on and uses the best one, instead of the first that reaches the threshold. |
| **Layer of the created text** | Puts the text on the current layer or on the layer of the geometry. |

## Notes
- If you select objects before starting the command, they are converted at once.
- Strokes are grouped into characters, lines of text, and blocks of text. Each recognised line becomes a multiline text object in a text style named after the font, and the strokes it replaces are erased.
- Groups below the threshold are left as geometry.
- A font is only compared when its SHX file is found (next to the drawing or in the fonts folder) or it is one of the built-in stroke fonts.
- When [`FILEDIA`](FILEDIA-System-Variable) is 0, **SEttings** asks on the command line: **Fonts**, **Threshold**, **Layer** (**Current** or **Geometry**) and **Bestfont**.
- The settings are saved and kept between sessions.
- The ribbon also has a **Recognition Settings** button that opens the settings.
- The conversion is one undo step.
