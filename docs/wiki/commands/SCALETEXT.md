---
summary: Changes the height of selected text and multiline text, each about its own base point.
related: JUSTIFYTEXT, TEXTALIGN, SCALE
ribbon: Annotate tab › Text panel (expanded)
---

## Usage
1. Select the text objects first, or start the command and select them. Only text and multiline text are picked.
2. `Enter a base point option for scaling`: press Enter for the default, type `E` to scale about each object's existing insertion point, or type a justification such as `MC` or `BL`.
3. `Specify new model height or [Paper height/Match object/Scale factor]`: type a height, or type an option.

## Options
| Option | What it does |
|---|---|
| **Existing** (`E`) | Scales each object about its own insertion point. |
| **Left**, **Center**, **Middle**, **Right**, **TL** … **BR** | Scales each object about that point of its box. |
| **Paper height** (`P`) | Sets the height on paper. Only annotative objects change; the current annotation scale gives the model height. Others are reported as ignored. |
| **Match object** (`M`) | Gives every object the height of a text object you pick. |
| **Scale factor** (`S`) | Multiplies each height by a factor. Type **Reference** (`R`) to set the factor as a new length divided by a reference length. Type the reference length or pick two points, then type the new length. |

## Notes
- The command line reports how many objects changed.
- The base point option and the heights you use are offered as defaults the next time. The first height offered comes from the current text size.
