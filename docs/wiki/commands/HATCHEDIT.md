---
summary: Changes the pattern, scale, angle, origin, style, properties or boundary of an existing hatch.
related: HATCH, GRADIENT, HATCHTOBACK, DRAWORDER, PROPERTIES
ribbon: Draw tab › Modify panel
---

## Usage
1. Select hatch: pick a hatch. If one hatch is already selected, this step is skipped.
2. The prompt shows the current pattern, scale and angle. Type an option, or press Enter for **Properties**.
3. Answer the prompts of that option. The change is applied to the hatch.

## Options
| Option | What it does |
|---|---|
| **Properties** (`P`, or Enter) | Asks for the pattern name (or `Solid`), then the scale, then the angle. |
| **Style** (`S`) | Sets island detection: **Ignore**, **Outer** or **Normal**. |
| **Origin** (`O`) | **Use current origin**, **Set new origin** (pick a point), or **Default to boundary extents** (bottom left, bottom right, top right, top left or center). Then asks whether to store it as the default origin. |
| **Color** (`CO`) | Sets the color: a color number or name, `ByLayer`, `ByBlock`, `.` for the current color, or **Truecolor** (`T`) for R,G,B. |
| **Layer** (`LA`) | Moves the hatch to another layer, or `.` for the current layer. |
| **Transparency** (`T`) | Sets transparency 0-90, `ByLayer` or `ByBlock`. |
| **Draw order** (`DR`) | **Do not change**, **Send to back**, **Bring to front**, **Behind boundary** or **In front of boundary**. |
| **Associate** (`AS`) | Links the hatch to a new boundary: pick an internal point or select boundary objects. |
| **Disassociate** (`DI`) | Removes the link between the hatch and its boundary. |
| **Annotative** (`AN`) | Makes the hatch annotative or not (**Yes**/**No**). |
| **Recreate boundary** (`B`) | Draws a new boundary around the hatch as a **Region** or **Polyline**, and asks whether to link the hatch to it. |
| **Separate hatches** (`H`) | Splits a hatch with several separate areas into one hatch per area. |

## Notes
- The command is also on the right-click menu of a selected hatch.
