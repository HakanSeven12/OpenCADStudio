---
summary: Draws section lines where an axis-aligned plane cuts one selected 3D solid.
related: SECTIONPLANE, SLICE, SECTIONCULL
---

## Usage
1. Select one 3D solid, or start the command and select it.
2. `cutting-plane axis [X / Y / Z]`: type the axis the plane is square to.
3. Type the offset along that axis, that is the coordinate where the plane lies.
4. Lines are drawn where the plane crosses the solid. The command line reports how many were made.

## Options
| Option | What it does |
|---|---|
| **X** (`X`) | Cuts with a plane at the given X coordinate. |
| **Y** (`Y`) | Cuts with a plane at the given Y coordinate. |
| **Z** (`Z`) | Cuts with a plane at the given Z coordinate. |

## Command-line form
You can type the whole request in one line, for example `SECTION Z 5`. Without an axis, Z is used: `SECTION 5`.

## Notes
- Exactly one 3D solid must be selected; otherwise the command stops with a message. Solids on locked layers do not count.
- The result is a set of lines, not a region. The solid is not changed.
- If the plane does not reach the solid, nothing is drawn.
- The new lines are one undo step.
