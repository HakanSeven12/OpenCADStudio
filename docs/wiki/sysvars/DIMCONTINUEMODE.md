---
summary: Controls whether continued and baseline dimensions take the layer and dimension style of the dimension they start from.
type: On/Off (0/1)
saved: Program settings
default: 1 (On)
related: DIMCONTINUE, DIMBASELINE, DIMLAYER
---

## Values
| Value | Meaning |
|---|---|
| 0 | New continued and baseline dimensions use the current dimension style and layer. |
| 1 | New continued and baseline dimensions use the layer and dimension style of the base dimension. |

## Notes
- Used by [`DIMCONTINUE`](DIMCONTINUE-Command) and [`DIMBASELINE`](DIMBASELINE-Command).
- You can also set it in the Options dialog: "Continued dimensions inherit the base dimension's layer and style".
