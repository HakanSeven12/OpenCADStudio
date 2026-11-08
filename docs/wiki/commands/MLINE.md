---
summary: Draws multilines, groups of parallel lines that follow a multiline style.
related: MLSTYLE, MLEDIT, PLINE
---

## Usage
1. The command line shows the current justification, scale and style. Specify the start point, or type an option.
2. Specify the next point. Keep adding points.
3. Press Enter to finish, or type `C` to close the multiline.

## Options
At the start point:

| Option | What it does |
|---|---|
| **Justification** (`J`) | **Top**, **Zero** or **Bottom**: whether the points you pick follow the top line, the middle (zero offset) or the bottom line. |
| **Scale** (`S`) | Multiplies the distance between the lines. Must not be zero. |
| **STyle** (`ST`) | Enter the name of a multiline style in the drawing. `?` lists the loaded styles. |

While adding points:

| Option | What it does |
|---|---|
| **Close** (`C`) | Closes the multiline back to its start. Available after three points. |
| **Undo** (`U`) | Removes the last point. |

## Notes
- The defaults come from the drawing's current multiline style, scale and justification (`CMLSTYLE`, `CMLSCALE`, `CMLJUST`).
- Edit crossings, corners and vertices afterwards with [`MLEDIT`](MLEDIT-Command). Manage styles with [`MLSTYLE`](MLSTYLE-Command).
