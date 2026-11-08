# Your First Drawing

This walk-through draws a simple plate with a hole, puts a dimension on it and saves it. It touches the tools you will use every day. Each step names the command, so you can type it as well as click it.

## 1. Start a drawing

Press **Ctrl+N** (`QNEW`). A new drawing opens from the default template, which you can choose under **Options › Files**. `NEW` asks which template to start from instead.

Check the units first: type `UNITS` and set the length type, precision and the insertion units (millimetres, inches and so on). The units shown in the status bar follow this setting.

## 2. Draw a rectangle

1. On the **Draw** tab, click <img src="images/icons/shapes/rect.svg" width="20" alt=""> **Rectangle**, or type `REC` and press Enter.
2. Type `0,0` and press Enter for the first corner.
3. Type `100,60` and press Enter for the opposite corner.

If the rectangle is off screen, double-click the middle mouse button, or type `Z` Enter `E` Enter (`ZOOM` › Extents).

## 3. Draw a circle in the middle

1. Click <img src="images/icons/circle/circle.svg" width="20" alt=""> **Circle**, or type `C` Enter.
2. For the centre, type `50,30` Enter.
3. For the radius, type `15` Enter.

## 4. Round the corners

1. Click <img src="images/icons/fillet.svg" width="20" alt=""> **Fillet**, or type `F` Enter.
2. Type `R` Enter, then `8` Enter to set the radius.
3. Type `P` Enter (Polyline) and pick the rectangle: all four corners are rounded at once.

## 5. Add a dimension

1. On the **Annotate** tab, click <img src="images/icons/dim_linear.svg" width="20" alt=""> **Linear**, or type `DLI` Enter.
2. Make sure object snap is on (**F3**). Click the two bottom corner points of the plate; the snap marker shows when you are on them.
3. Move the cursor down and click to place the dimension line.

## 6. Put things on layers

Open the Layer Manager with <img src="images/icons/layers/panel.svg" width="20" alt=""> **Layers** (`LA`). Create a layer named `DIMENSIONS`, give it a colour, then select the dimension and pick the new layer in the **Layers** list on the ribbon. See [Layers and Object Properties](Layers-and-Properties).

## 7. Save

Press **Ctrl+S** (`SAVE`). The first save asks for a name, a folder and the file type (DWG or DXF, and which version). See [Files and Formats](Files-and-Formats) for the versions.

## What next

- Learn to place points exactly: [Precision Drawing](Precision-Drawing).
- Learn the rest of the screen: [The User Interface](User-Interface).
- Put the drawing on a sheet and print it: [Layouts and Viewports](Layouts-and-Viewports), then [Plotting and Publishing](Plotting-and-Publishing).
