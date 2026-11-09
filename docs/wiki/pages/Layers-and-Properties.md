# Layers and Object Properties

Every object has a set of properties: its layer, colour, linetype, lineweight, transparency and plot style. Most drawings set these through layers: an object takes the layer's properties (ByLayer), and changing the layer changes all its objects at once.

## The Layer Manager

<img src="images/icons/layers/panel.svg" width="24" alt=""> `LAYERS` (`LA`) opens the Layer Manager.

![The Layer Manager](images/screens/layer-manager.png)

(`LAYER` typed on its own works at the command line: it lists the layers and takes subcommands such as `NEW`, `ON`, `OFF` and `FREEZE`.) Each row is a layer:

| Column | Meaning |
|---|---|
| Status | A tick marks the current layer, the one new objects go on. |
| Name | The layer name. |
| On | Off layers are hidden and do not plot, but still count for regeneration. |
| Freeze | Frozen layers are hidden, do not plot and are skipped entirely, which keeps big drawings fast. The current layer cannot be frozen. |
| Lock | Objects on locked layers stay visible and snappable but cannot be selected or changed; they show faded. |
| Plot | Whether the layer prints. |
| Color · Linetype · Lineweight · Transparency | What ByLayer objects on the layer look like. |
| Plot style | The named plot style, in drawings that use named plot styles. |
| New VP Freeze | Freezes the layer in every new layout viewport. |
| Description | A note for the layer. |

On a layout, extra columns let you freeze a layer or override its colour, linetype, lineweight and transparency **in the current viewport only** (VP Freeze and the VP overrides).

Layer `0` always exists and cannot be deleted or renamed. Objects inside blocks that are drawn on layer 0 take the layer of the block reference.

**Creating and deleting.** Use the New and Delete buttons, or `-LAYER` at the command line. A layer that still has objects cannot be deleted from the manager; `LAYDEL` deletes a layer together with its objects, and `LAYMRG` moves the objects to another layer first.

### Filters and groups

The tree on the left of the Layer Manager narrows the list:

- **Property filters** show the layers that match rules, for example every layer whose name starts with `A-` and that is frozen.
- **Groups** hold a hand-picked list of layers.
- Filters and groups can be nested, and are saved in the drawing.
- The search box filters the list by name.

### Layer states

<img src="images/icons/layers/laycur.svg" width="20" alt=""> `LAYERSTATE` saves the on, freeze, lock, plot, colour and other settings of every layer under a name, so you can switch between, say, a "Furniture plan" and an "Electrical plan" of the same drawing. Layer states are saved in the drawing and can be exported to and imported from other drawings.

### Importing layers

The Layer Manager's **Import** brings layer definitions in from another drawing. `LAYTRANS` maps the layer names of a drawing to a standard: pick the layers, pick the target names, and the objects move across.

## Layer tools

The **Layers** panel on the Draw tab works on the layers of objects you pick, without opening the manager:

| | Tool | Command | What it does |
|---|---|---|---|
| <img src="images/icons/layers/laymcur.svg" width="20" alt=""> | Make Object's Layer Current | `LAYMCUR` | Picks an object and makes its layer current. |
| <img src="images/icons/layers/laymatch.svg" width="20" alt=""> | Match Layer | `LAYMCH` | Moves objects to the layer of another object. |
| <img src="images/icons/layers/laycur.svg" width="20" alt=""> | Change to Current Layer | `LAYCUR` | Moves objects to the current layer. |
| <img src="images/icons/layers/copytolayer.svg" width="20" alt=""> | Copy to Layer | `COPYTOLAYER` | Copies objects to another layer. |
| <img src="images/icons/layers/layiso.svg" width="20" alt=""> | Isolate | `LAYISO` | Keeps only the layers of the selected objects; the others are turned off or locked and faded. |
| <img src="images/icons/layers/layuniso.svg" width="20" alt=""> | Unisolate | `LAYUNISO` | Undoes the last isolate. |
| <img src="images/icons/layers/layoff.svg" width="20" alt=""> | Off | `LAYOFF` | Turns off the layer of each object you pick. |
| <img src="images/icons/layers/layon.svg" width="20" alt=""> | Turn All On | `LAYON` | Turns every layer on. |
| <img src="images/icons/layers/layfrz.svg" width="20" alt=""> | Freeze | `LAYFRZ` | Freezes the layer of each object you pick. |
| <img src="images/icons/layers/laythw.svg" width="20" alt=""> | Thaw All | `LAYTHW` | Thaws every layer. |
| <img src="images/icons/layers/laylck.svg" width="20" alt=""> | Lock | `LAYLCK` | Locks the layer of an object. |
| <img src="images/icons/layers/layulk.svg" width="20" alt=""> | Unlock | `LAYULK` | Unlocks the layer of an object. |
| <img src="images/icons/layers/layvpfrz.svg" width="20" alt=""> | Freeze in Viewport | `VPLAYER` | Freezes layers in the current layout viewport only. |
| <img src="images/icons/layers/laywalk.svg" width="20" alt=""> | Layer Walk | `LAYWALK` | Shows the layers one at a time (or a few at a time) to see what is on each. |

## Colour

Colours can be:

- an **index colour** (1 to 255), the classic palette;
- a **true colour** (any RGB value);
- a colour from a **colour book**;
- **ByLayer** (take the layer's colour) or **ByBlock** (take the colour of the block reference).

Set the colour for new objects with `COLOR`, or in the **Properties** panel of the Draw tab.

## Linetypes

<img src="images/icons/bylayer.svg" width="20" alt=""> `LINETYPE` (`LT`) lists the loaded linetypes, sets the current one and loads more from a linetype file (dashed, centre, hidden and many others, including linetypes with text and shapes). `-LINETYPE` also creates, renames and deletes linetypes from the command line.

| Variable | Scales |
|---|---|
| `LTSCALE` | Every linetype in the drawing. |
| `CELTSCALE` | New objects, on top of LTSCALE. |
| `MSLTSCALE` | Model-space linetypes by the annotation scale. |
| `PSLTSCALE` | Linetypes in layout viewports by the viewport scale, so dashes look the same on paper at every scale. |

To add your own linetypes, put `.lin` files in the `linetypes` folder of your settings folder or in the support folder (**Options › Files**) and restart the program.

## Lineweight

<img src="images/icons/attributes/lineweight.svg" width="20" alt=""> Lineweights are printed widths, from 0.00 mm up. They show on screen only when <img src="images/icons/status/lwt.svg" width="20" alt=""> **Show Lineweight** is on in the status bar (`LWDISPLAY`). `LWDEFAULT` is the width that **Default** stands for.

## Transparency

<img src="images/icons/attributes/transparency.svg" width="20" alt=""> Transparency runs from 0 (opaque) to 90. It shows on screen when <img src="images/icons/status/transparency.svg" width="20" alt=""> **Show Transparency** is on, and can be plotted.

## Changing properties

- Select objects and change their properties in the **Properties** palette (**Ctrl+1**) or on the ribbon.
- <img src="images/icons/match_prop.svg" width="20" alt=""> `MATCHPROP` (`MA`) copies the properties of one object to others.
- <img src="images/icons/modify_setbylayer.svg" width="20" alt=""> `SETBYLAYER` resets properties to ByLayer.
- `CHPROP` changes properties at the command line.
