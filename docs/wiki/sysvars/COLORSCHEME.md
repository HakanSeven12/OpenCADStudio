---
summary: Sets the color theme of the program window.
type: String
saved: Program settings
default: Oxocarbon
related: COLORTHEME, OPTIONS
---

## Values
| Value | Meaning |
|---|---|
| DARK, LIGHT, DRACULA, NORD, SOLARIZED_LIGHT, SOLARIZED_DARK, GRUVBOX_LIGHT, GRUVBOX_DARK, TOKYONIGHT, TOKYONIGHTSTORM, TOKYONIGHTLIGHT, KANAGAWAWAVE, KANAGAWADRAGON, KANAGAWALOTUS, MOONFLY, NIGHTFLY, OXOCARBON, FERRA | A built-in theme. |
| SYSTEM | Follow the light or dark setting of the operating system or browser. It is checked again each time you choose it. |

## Notes
- Type `COLORSCHEME` to see the list and the current theme, then enter a theme name. `COLORSCHEME ?` lists the themes.
- 0 and 1 are not accepted here; use [`COLORTHEME`](COLORTHEME-System-Variable) for a plain dark or light choice.
