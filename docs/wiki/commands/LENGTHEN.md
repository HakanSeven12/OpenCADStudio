---
summary: Changes the length of lines, arcs, elliptical arcs, splines and polylines, or the included angle of arcs.
related: TRIM, EXTEND, BREAK, STRETCH
---

## Usage
1. At `Select an object to measure or [Delta/Percent/Total/Dynamic]:`, pick an object to see its length, or type an option.
2. Enter the value the option asks for.
3. At `Select an object to change or [Undo]:`, pick objects near the end you want to change. Each pick changes that object.
4. Press Enter or Esc to finish.

## Options
| Option | What it does |
|---|---|
| **Delta** (`DE`) | Adds the length you enter. A negative value shortens the object. |
| **Percent** (`P`) | Sets the length to a percentage of the current length. |
| **Total** (`TO`) | Sets the total length you enter. |
| **Dynamic** (`DY`) | Pick a line, arc or elliptical arc, then pick the new end point. |
| **Angle** (`A`) | After **Delta** or **Total**, changes an arc by an angle instead of a length. |
| **Undo** (`U`) | Undoes the last change made in this command. |

## Notes
- The end nearest the point you pick is the end that changes.
- Press Enter at the first prompt to reuse the last option. Each value prompt offers the last value as its default.
- Percent and total values must be greater than zero. A total angle must be less than 360 degrees.
