# Layouts and Viewports

You draw at full size in **model space**. To print, you place views of the model on a **layout**: a sheet of paper (paper space) with a title block, notes and one or more **layout viewports**, each showing the model at a scale.

The **Model** tab and one tab per layout sit under the drawing area. Click a tab to switch, right-click it to create, rename, copy, move or delete layouts.

## Creating a layout

1. Right-click a layout tab › **New layout**, or use `LAYOUT`.
2. Open the new layout and set the paper with <img src="images/icons/pagesetup.svg" width="20" alt=""> **Page Setup** (`PAGESETUP`): printer, paper size, orientation, plot style table. See [Plotting and Publishing](Plotting-and-Publishing).
3. Insert the title block, usually as a block.
4. Create viewports.

`LAYOUTCREATEVIEWPORT` decides whether a new layout starts with one viewport. `MVSETUP` sets up a layout from the command line.

## Layout viewports

<img src="images/icons/viewport.svg" width="24" alt=""> `MVIEW` (`MV`) creates viewports on a layout: by two corners, **Fit** to the printable area, as a **Polygonal** outline, from an existing closed **Object**, or with **Insert view** from a named model view.

To set what a viewport shows:

1. Double-click inside the viewport to work in model space through it (`MSPACE`). Its border becomes thick.
2. Pan and zoom to the area you want.
3. Set the scale from the status bar list or in Properties (**Standard scale**, or a custom scale).
4. Double-click outside the viewport to go back to paper space (`PSPACE`).

**Lock the viewport** (status bar, or **Display locked** in Properties) so that a zoom inside it cannot change its scale by accident.

Other viewport tools:

| Command | What it does |
|---|---|
| `VPCLIP` | Gives a viewport a polygonal or object outline, or removes it. |
| `VPLAYER` | Freezes layers in chosen viewports only. The Layer Manager has VP Freeze and VP override columns for the same purpose; see [Layers and Object Properties](Layers-and-Properties). |
| `CHSPACE` | Moves objects between paper space and the model space of a viewport, keeping their printed size. |
| `VPSYNC` / `SYNCPVIEWPORTS` | Lines up the views of several viewports with a master viewport. |
| **Ctrl+R** | Moves to the next viewport. |

Viewports are objects on the layout: copy, move, stretch and erase them, and put them on a layer that does not plot to hide their borders.

## Scales and linetypes on sheets

- `PSLTSCALE` = 1 makes dashed linetypes look the same in every viewport, whatever its scale.
- Annotative text, dimensions and hatches show at the right size in each viewport; see [Annotation Scaling](Annotation-Scaling).
- Dimensions placed on the layout can measure model objects through a viewport and stay associative.

## Paper units

A layout has its own units, taken from its page setup: millimetres or inches. The scale of a viewport compares paper units to model units, so check both when a scale looks wrong.

## The Layout tab

While a layout is active, the **Layout** tab shows on the ribbon with the Viewport tools (create, clip, lock, scale) and the Plot tools.
