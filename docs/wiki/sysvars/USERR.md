---
summary: Stores your own real numbers in five registers, USERR1 to USERR5.
type: Real
saved: Drawing
default: 0
related: USERI, FIELD, QUICKCALC
---

Each register takes any real number.

## Notes
- Type a register's name, for example `USERR3`, to read or change it, or use [`SETVAR`](SETVAR-Command).
- Type `USERR`, then the register number (1 to 5), then the value. Or type it in one line, for example `USERR 3 1.5`.
- Fields and [`QUICKCALC`](QUICKCALC-Command) can read the registers (USERR1 to USERR5).
- `USERS1` to `USERS5` hold text the same way. They stay with the open drawing and are not saved.
- Open CAD Studio itself does not use the values.
