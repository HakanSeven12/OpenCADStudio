# Tutorials

Three short lessons, each building on the one before. Every step names the command, so you can type it as well as click it. All files are saved as DWG.

1. [A 2D part with dimensions](#1-a-2d-part-with-dimensions)
2. [Putting the part on a sheet](#2-putting-the-part-on-a-sheet)
3. [A simple 3D part](#3-a-simple-3d-part)

---

## 1. A 2D part with dimensions

A 100 × 60 plate with two holes, rounded corners and dimensions.

**Start a new drawing.** Press **Ctrl+N** (`QNEW`). A new drawing measures in metres; for this millimetre part, open `UNITS` and set the insertion units to **Millimeters** (see [Drawing Setup and Units](Drawing-Setup-and-Units)).

**Draw the outline and the holes.**

1. `RECTANG`, then type `0,0` Enter and `100,60` Enter.
2. `CIRCLE`, then `20,30` Enter for the centre and `8` Enter for the radius.
3. `CIRCLE` again: `80,30` Enter, `8` Enter.
4. Show it all: `ZOOM` › **Window** around the part, or double-click the middle mouse button.

![The plate and two holes](images/screens/tutorial-2d-1.png)

**Round the corners.**

1. `FILLET`, then `R` Enter and `8` Enter to set the radius.
2. `P` Enter (**Polyline**) and click the rectangle. All four corners are rounded at once.

![Rounded corners](images/screens/tutorial-2d-2.png)

**Dimension it on a layer of its own.**

1. Create a layer for the dimensions and make it current: in the Layer Manager (`LA`) click **New**, name it `DIMENSIONS`, give it a colour and click **Set Current**. At the command line the same is `-LAYER`, `M`, `DIMENSIONS`.
2. `DIMLINEAR`: click the bottom-left and bottom-right corners (object snap **Endpoint**, or type `0,0` and `100,0`), then place the line below the part.
3. `DIMLINEAR` again for the height (`0,0` and `0,60`), placed to the left.
4. `DIMLINEAR` between the two hole centres (`20,30` and `80,30`, or snap **Center**), placed above.
5. `DIMDIAMETER`: click a hole, then place the dimension.

![Dimensions on their own layer](images/screens/tutorial-2d-3.png)

**Save.** **Ctrl+S**, name the file `bracket`, keep the type **DWG**.

![The saved drawing](images/screens/main-window.png)

---

## 2. Putting the part on a sheet

1. Click the **Layout1** tab under the drawing. The sheet comes with one viewport that shows the model.
2. To change what the viewport shows, double-click inside it, pan and zoom, then double-click outside it. Set an exact scale by selecting the viewport and choosing a **Standard scale** in Properties (Ctrl+1); then set **Display locked** to Yes so a zoom cannot change it.
3. Press **Ctrl+P** (`PLOT`). Choose the printer, or PDF output, the paper size and **What to plot: Layout**, and click **Print**.

![A layout with its viewport](images/screens/tutorial-layout-1.png)

![The Plot dialog](images/screens/plot-dialog.png)

See [Layouts and Viewports](Layouts-and-Viewports) and [Plotting and Publishing](Plotting-and-Publishing) for page setups, plot styles and publishing several sheets.

---

## 3. A simple 3D part

A block with a hole through it.

1. **Ctrl+N** for a new drawing.
2. `BOX`: first corner `0,0,0`, other corner `80,50,0`, height `30`.
3. `CYLINDER`: centre `40,25,0`, radius `10`, height `30`.
4. Look at it in 3D: click a corner of the ViewCube, or use the **View** tab › **Isometric**. Choose a shaded visual style with `VSCURRENT` or the **View** tab.

![Box and cylinder](images/screens/tutorial-3d-1.png)

5. `SUBTRACT`: select the box and press Enter, then select the cylinder and press Enter. The cylinder is cut out of the box.
6. Save as `part.dwg`.

![The finished part](images/screens/tutorial-3d-2.png)

Continue with [3D Modelling](3D-Modelling) for extrusions, sweeps, lofts, edge fillets and sections.
