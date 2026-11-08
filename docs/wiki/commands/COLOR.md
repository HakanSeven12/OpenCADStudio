---
summary: Sets the colour given to new objects.
related: BYLAYER, LAYER, PROPERTIES, MATCHPROP
---

## Usage
1. Type `COLOR`.
2. At the prompt, type a colour: `ByLayer`, `ByBlock`, a colour index from 1 to 255, or a colour name (`red`, `yellow`, `green`, `cyan`, `blue`, `magenta`, `white`).
3. Press Enter. The command line confirms the new object colour.

Press Enter without a value to see the current object colour.

## Command-line form
You can give the value on the same line, for example `COLOR 1` or `COLOR ByLayer`. This suits scripts.

## Notes
- The colour applies to objects you draw from now on. Existing objects keep their colour.
- The colour control on the ribbon follows the new value.
- An unknown value prints the accepted forms and changes nothing.
