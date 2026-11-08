---
summary: Saves your keyboard shortcuts to a text file.
related: CUIIMPORT, CUILOAD, CUI
ribbon: Manage tab › Customization panel
web: Not available in the web app.
---

## Usage
1. Run `CUIEXPORT`.
2. Type the path of the file to save the shortcuts to and press Enter.
3. The command line reports how many shortcuts were written.

## Command-line form
`CUIEXPORT <path>` writes the file at once. This suits scripts.

## Notes
- The file has one shortcut per line: the key, a space, then the command, sorted by key.
- Load the file again with [`CUIIMPORT`](CUIIMPORT-Command).
- Ribbon panels and aliases are not part of this file.
