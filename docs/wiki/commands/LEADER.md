---
summary: Draws a leader line with an arrowhead, and an optional text annotation at its end.
related: QLEADER, MLEADER, MTEXT, DIMSTYLE
ribbon: Draw tab › Annotation panel
---

## Usage
1. At `Specify leader start point:`, pick the point the arrow points to.
2. At `Specify next point:`, pick the next point of the leader line.
3. Pick more points, or type an option. Press Enter to go on to the annotation.
4. At `Enter first line of annotation text or <options>:`, type a line of text and press Enter. Type more lines, then press Enter on an empty line to finish.
5. Or press Enter at the first text line to see the annotation options.

## Options
| Option | What it does |
|---|---|
| **Annotation** (`A`) | Goes on to the annotation text. Same as pressing Enter. |
| **Format** (`F`) | Asks for the leader format, see below. |
| **Undo** (`U`) | Removes the last point. |

**Format** offers these choices at `Enter leader formatting option [Spline/Straight/Arrow/None] <exit>:`. Press Enter to go back to picking points.

| Option | What it does |
|---|---|
| **Spline** (`S`) | Draws the leader as a smooth curve. |
| **Straight** (`ST`) | Draws the leader as straight segments. |
| **Arrow** (`A`) | Draws an arrowhead at the start point. |
| **None** (`N`) | Draws no arrowhead. |

At `Enter an annotation option [None/Mtext]:` you can choose:

| Option | What it does |
|---|---|
| **None** (`N`) | Finishes the leader without text. |
| **Mtext** (`M`) | Places an empty multiline text at the end of the leader and opens the text editor so you can type it. |

## Notes
- The arrow size, text style and text height come from the current dimension style.
- The text is linked to the leader, so they stay together when you edit them.
- To draw multileaders, use [`MLEADER`](MLEADER-Command).
