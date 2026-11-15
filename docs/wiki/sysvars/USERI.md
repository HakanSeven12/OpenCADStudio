---
summary: Stores your own whole numbers in five registers, USERI1 to USERI5.
type: Integer
saved: Drawing
default: 0
related: USERR, FIELD, QUICKCALC
---

Each register takes a whole number from -32768 to 32767.

## Notes
- Type a register's name, for example `USERI1`, to read or change it, or use [`SETVAR`](SETVAR-Command).
- Type `USERI`, then the register number (1 to 5), then the value. Or type it in one line, for example `USERI 1 42`.
- Fields and [`QUICKCALC`](QUICKCALC-Command) can read the registers (USERI1 to USERI5).
- `USERS1` to `USERS5` hold text the same way. They stay with the open drawing and are not saved.
- Open CAD Studio itself does not use the values.
