---
summary: Imports named page setups from another drawing or template into the current drawing.
related: PAGESETUP, PLOT
---

## Usage
1. Pick a drawing or template (DWG, DXF or DWT) in the file browser.
2. The Plot dialog opens with the list of page setups found in that file, all ticked.
3. Untick the ones you do not want (**All** ticks or unticks every one) and click **Import**.

The command line reports how many page setups were imported and how many existing ones were replaced.

## Command-line form
`-PSETUPIN` works the same way. Both forms also take arguments, which suits scripts:
- `PSETUPIN <file>` opens the chooser for that file without the file browser.
- `PSETUPIN <file> *` imports every page setup of the file at once.
- `PSETUPIN <file> name1,name2` imports only the named setups. A name that is not found is reported.

## Notes
- A page setup with the same name as an existing one replaces it.
- The first imported page setup becomes the one selected in the Plot dialog.
- The import is one undo step.
