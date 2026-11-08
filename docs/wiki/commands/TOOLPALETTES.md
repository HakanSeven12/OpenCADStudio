---
summary: Opens the Tool Palettes panel, where you keep blocks on palette tabs and insert them with one click.
related: TOOLPALETTESCLOSE, INSERT, BLOCK
ribbon: View tab › Palettes panel; Manage tab › Customization panel
---
## Usage
1. Run `TOOLPALETTES`. The Tool Palettes panel opens in the dock.
2. Click a tool to insert its block into the current drawing.

## Panel
| Control | What it does |
|---|---|
| **Search tools** | Filters the tools on the current palette by name. |
| **Add Selected** | Adds the selected block references to the current palette. Their scale and rotation are stored with the tool. |
| Block list and **+** | Picks a block defined in the current drawing and adds it to the palette. |
| **New palette name** and **+** / **−** | Creates a new palette tab, or deletes the current one (the last tab cannot be deleted). |
| Palette tabs | The vertical tabs at the side switch between palettes. |
| Tool row buttons | Move a tool up or down, or remove it. |
| **Insertion point** | When on, you pick the insertion point after clicking a tool. |
| **Explode** | When on, the block is exploded after insertion. |

## Notes
- A tool stores a full copy of its block definition, so you can insert it into any drawing, also after a restart.
- Dynamic and annotative blocks show a badge next to their name.
- The panel's open state and its palettes are saved with your settings.
- You can open or close the panel while another command is running.
