---
summary: Evaluates an arithmetic or geometric expression at the command line and prints the result.
related: QUICKCALC
---

## Usage
1. Run `CAL`.
2. At "CAL  expression  (e.g. (2+3)*4):", type an expression and press Enter. Spaces inside the expression are allowed.
3. The result is printed as `= <value>`.

You can also type the expression on the same line, for example `CAL (2+3)*4`.

## Notes
- Operators: `+ - * /`, `^` for powers, and parentheses.
- Points are written in square brackets, for example `[1,2,0]`, and can be added, subtracted and scaled.
- Constants and memory: `pi`, and `ans` for the last result. Store a value with `$name = expression` and use it later as `$name`.
- Unit suffixes convert to drawing units: `mm`, `cm`, `m`, `km`, `in` or `"`, `ft` or `'`. Angle suffixes: `deg`, `rad`, `grad`.
- Functions include `sin`, `cos`, `tan` and their inverses, `sqrt`, `cbrt`, `root`, `abs`, `round`, `floor`, `ceil`, `fact`, `ln`, `log`, `exp`, `pow`, `min`, `max`, `d2r`, `r2d`, and for points `dist`, `ang`, `mid`, `xof`, `yof`, `zof`. `getvar` reads a system variable.
- `CAL` uses the same calculator as the [`QUICKCALC`](QUICKCALC-Command) palette.
- An invalid expression prints an error that starts with "CAL:".
