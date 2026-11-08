---
summary: Creates a 3D solid or surface by extruding closed or open 2D profiles.
related: PRESSPULL, REVOLVE, SWEEP, LOFT, THICKEN, CREATESKETCH
ribbon: Model tab › Create panel
---

## Usage
1. Select the profiles first, or start the command and pick them at `Select objects to extrude or [MOde]:`. Press Enter when done.
2. At `Specify height or [Direction/Path/Taper angle/Expression]`, move the cursor and click, or type a height. A preview follows the cursor.
3. The extrusion is created.

Press Enter at the height prompt to reuse the last height (shown in `< >`).

## Options
| Option | What it does |
|---|---|
| **Mode** (`MO`) | At the selection prompt: choose **Solid** or **Surface**. The choice is remembered. |
| **Direction** (`D`) | Pick a start point and an end point. The profile is extruded along that vector. |
| **Path** (`P`) | Pick a curve to extrude along. |
| **Taper angle** (`T`) | Type a taper angle (less than 90 degrees in either direction). Available at the height and path prompts. |
| **Expression** (`E`) | Type a math expression for the height or the taper angle. |

## Notes
- A closed profile makes a solid in Solid mode. An open profile always makes a surface.
- The height is measured along the profile's normal. A negative height extrudes the other way.
- The profile's own plane is used, so profiles do not have to lie in the current UCS.
- The [`DELOBJ`](DELOBJ-System-Variable) setting decides whether the profiles are kept after the extrusion.
- After [`FINISHSKETCH`](FINISHSKETCH-Command), type `P` (Previous) at the selection prompt to pick everything you just drew.
- Objects on locked layers cannot be extruded.
