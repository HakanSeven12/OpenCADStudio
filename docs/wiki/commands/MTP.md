---
summary: Point-entry modifier that finds the point midway between two points you pick.
related: M2P, FROM, OSNAP
---

## Usage
1. Start a command that asks for a point, such as `LINE` or `MOVE`.
2. At the point prompt, type `MTP`.
3. Specify first point of mid.
4. Specify second point of mid. A rubber-band line and a small triangle mark the midpoint while you move the cursor.
5. The midpoint goes back to the command as the answer to its prompt.

## Notes
- `MTP` only works while a prompt asks for a point. It also works while you drag a grip, so the grip lands on the midpoint.
- Press Esc or Enter before the second point to cancel it.
- [`M2P`](M2P-Command) is the same modifier.
