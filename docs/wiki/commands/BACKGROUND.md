---
summary: Sets the background color of the drawing area for model space or for the current layout.
related: OPTIONS, COLORSCHEME
---

## Usage
1. Type a color name, a preset, or three numbers from 0 to 255 for red, green and blue.
2. Press Enter.

You can also type the value on the same line, for example `BACKGROUND 30 30 30`, `BACKGROUND 30,30,30` or `BACKGROUND WHITE`.

## Options
| Option | What it does |
|---|---|
| **Theme** (`THEME`) | Model space follows the light or dark theme of the program. |
| **Classic** (`CLASSIC`) | Model space uses the classic dark CAD background. |
| **Default** (`DEFAULT`) | Resets the background of the current space to its default. |
| **Black**, **DarkGray**, **Gray**, **LightGray**, **White** | Uses that preset color. |
| **R G B** | Uses your own color, for example `40 44 52`. |
| **Desk** (`DESK`) | Followed by R G B or `DEFAULT`: sets or resets the color around the paper sheet in a layout. |

## Notes
- In model space the command changes the model space background. In a layout it changes the layout background.
- The color is saved in your settings, so it is kept after a restart and used for new drawings.
- This is a display setting only. It is not saved in the drawing file.
