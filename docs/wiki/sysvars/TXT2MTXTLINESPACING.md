---
summary: Sets whether TXT2MTXT gives the combined text uniform line spacing.
type: On/Off (0/1)
saved: Program settings
default: 1 (Uniform)
related: TXT2MTXT, TXT2MTXTCOMBINEMTEXT, TXT2MTXTWORDWRAP
---

## Values
| Value | Meaning |
|---|---|
| 0 | Each paragraph keeps the gap its text object had to the next one |
| 1 | Uniform line spacing |

## Notes
- Only used while [`TXT2MTXTCOMBINEMTEXT`](TXT2MTXTCOMBINEMTEXT-System-Variable) is 1.
- While [`TXT2MTXTWORDWRAP`](TXT2MTXTWORDWRAP-System-Variable) is 1, spacing is uniform whatever this value is.
- The Settings window of [`TXT2MTXT`](TXT2MTXT-Command) sets it too.
