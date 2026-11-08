---
summary: Ties selected dimensions to points or objects in the drawing, so they update when that geometry changes.
related: DIMDISASSOCIATE, DIMLINEAR, DIMANGULAR
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Select the dimensions, or type an option, then press Enter.
2. The dimensions are taken one at a time. A blue cross marks the definition point being asked for.
3. Answer each prompt by picking a point (object snaps help) or an object. Press Enter to keep a point as it is and go to the next one.

What is asked depends on the dimension type:

| Dimension | Prompts |
|---|---|
| Linear, aligned | First extension line origin, then second extension line origin. Or **Select object** (`S`) and pick one line or arc to use its two ends. |
| Angular (three points) | Angle vertex, first angle endpoint, second angle endpoint. Or **Select arc or circle** (`S`) and pick the arc. |
| Angular (two lines) | Select the first line, then the second line. |
| Radius, diameter | Select the arc or circle. |
| Ordinate | Feature location. |

## Options
| Option | What it does |
|---|---|
| **Disassociated** (`D`) | At the first prompt, picks every dimension in the current space that has no association yet. |

## Notes
- Only dimensions in the current space that you are allowed to edit are used; dimensions on locked layers are skipped.
- [`DIMDISASSOCIATE`](DIMDISASSOCIATE-Command) removes the links again.
