---
summary: Lists the drawing's layers on the command line, or changes layers with a typed subcommand such as `LAYER NEW <name>`.
related: LAYERS, LAYERSTATE, LAYMCUR, RENAME, PURGE
---

`LAYER` works on the command line. To open the Layer Manager palette, use [`LAYERS`](LAYERS-Command).

## Usage
1. Type `LAYER` and press Enter.
2. The command line lists every layer with its state: on, off, frozen or locked.

To change layers, type a subcommand after `LAYER`, for example `LAYER FREEZE Walls Doors`.

## Options
| Option | What it does |
|---|---|
| **List** (`LIST` or `?`) | Lists every layer with its state. Same as `LAYER` on its own. |
| **New** (`NEW` or `N`) `<name>` | Creates a new layer. Reports an error if the name is already used. |
| **On** (`ON`) `<names>` | Turns the named layers on and thaws them. |
| **Off** (`OFF`) `<names>` | Turns the named layers off. |
| **Freeze** (`FREEZE` or `FR`) `<names>` | Freezes the named layers. |
| **Thaw** (`THAW` or `TH`) `<names>` | Thaws the named layers. |
| **Lock** (`LOCK` or `LO`) `<names>` | Locks the named layers. |
| **Unlock** (`UNLOCK` or `UL`) `<names>` | Unlocks the named layers. |
| **Color** (`COLOR` or `C`) `<name> <number>` | Sets the color of one layer to a color number. |
| **Set** (`SET`, `S` or `CURRENT`) `<name>` | Makes the named layer current. |

## Command-line form
`-LAYER` asks for everything at the command line and never opens a dialog, so it suits scripts. It asks for an option, then for the values that option needs, and then asks for the next option. Press Enter on an empty option prompt to finish.

1. Enter an option: `?`, `Make`, `Set`, `New`, `Rename`, `ON`, `OFF`, `Color`, `Ltype`, `LWeight`, `TRansparency`, `Description`, `MATerial`, `Plot`, `Pstyle`, `Freeze`, `Thaw`, `LOck` or `Unlock`.
2. Answer the prompts for that option.
3. Enter the next option, or press Enter to finish.

| Option | What it asks |
|---|---|
| **?** | Layer names to list (default `*`). |
| **Make** | A name. The layer is created if needed and becomes the current layer. |
| **Set** | The name of an existing layer to make current. |
| **New** | A list of names for new layers. |
| **Rename** | The old name, then the new name. |
| **Color** | A color name or number (1-255), then the layers to change. |
| **Ltype** | A loaded linetype name (default Continuous), then the layers. |
| **LWeight** | A lineweight from 0.0mm to 2.11mm, then the layers. |
| **TRansparency** | A value from 0 to 100, then the layers. |
| **Description** | A description text, then the layers. |
| **MATerial** | A material name (default ByLayer), then the layers. |
| **Plot** | Plot or No plot, then the layers. |
| **Pstyle** | A plot style name (default Normal), then the layers. |
| **ON / OFF / Freeze / Thaw / LOck / Unlock** | The layers to change. |

The answers can also follow the command on one line, for example `-LAYER M WALLS C 1 WALLS`.

## Notes
- In `-LAYER`, a layer list can hold several names separated by commas, and wildcards such as `A*`. When it asks for a layer list, Enter uses the current layer.
- `-LAYER` will not freeze the current layer. It reports this and changes the other layers in the list.
- An invalid color, lineweight or transparency value is refused, and the prompt asks again.
- Layer changes can be undone.
