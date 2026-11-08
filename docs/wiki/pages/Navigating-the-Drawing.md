# Navigating the Drawing

Moving around a drawing changes only what you see, never the drawing itself. Zoom and pan as often as you like, also in the middle of another command.

## Mouse

| Action | Result |
|---|---|
| Turn the wheel | Zoom in or out about the cursor. |
| Hold the middle button and drag | Pan. |
| Double-click the middle button | Zoom to the extents of the drawing. |
| Shift + middle button drag | Orbit the model in 3D. |

`ZOOMFACTOR` sets how far one notch of the wheel zooms; `ZOOMWHEEL` reverses the direction. **Options › User Preferences** has the same settings with a slider.

On a Mac trackpad, two fingers pan and a pinch zooms.

## Zoom

<img src="images/icons/zoom_ext.svg" width="20" alt=""> `ZOOM` (`Z`) changes the magnification. Its options:

| Option | Shows |
|---|---|
| **Window** (`W`) | The area inside a rectangle you drag. |
| **Extents** (`E`) | Everything in the drawing, as large as it fits. |
| **All** (`A`) | The drawing limits or the extents, whichever is larger. |
| **Previous** (`P`) | The view before the last zoom or pan. |
| **Object** (`O`) | The objects you select. |
| **Dynamic** (`D`) | A view box you move and size over the whole drawing. |
| **In** / **Out** | Twice or half the current magnification. |
| a number | A scale factor: `2` doubles, `0.5` halves. |

The **View** tab and the navigation bar have buttons for <img src="images/icons/zoom_window.svg" width="20" alt=""> Window, <img src="images/icons/zoom_ext.svg" width="20" alt=""> Extents, <img src="images/icons/zoom_in.svg" width="20" alt=""> In and <img src="images/icons/zoom_out.svg" width="20" alt=""> Out.

## Pan

<img src="images/icons/pan.svg" width="20" alt=""> `PAN` (`P`) moves the view with the left mouse button until you press Esc. The middle button does the same at any time.

## Orbit and 3D views

<img src="images/icons/orbit.svg" width="20" alt=""> `3DORBIT` (`3O`) turns the view around the model. Shift with the middle button does the same without starting a command.

The <img src="images/icons/viewcube.svg" width="20" alt=""> **ViewCube** in the corner of the drawing shows which way you are looking. Click a face, edge or corner to look from there; drag it to orbit. `NAVVCUBE` shows or hides it.

Preset views are on the **View** tab:

| | View |
|---|---|
| <img src="images/icons/view_top.svg" width="20" alt=""> | Top (plan) |
| <img src="images/icons/view_front.svg" width="20" alt=""> | Front |
| <img src="images/icons/view_right.svg" width="20" alt=""> | Right |
| <img src="images/icons/view_iso.svg" width="20" alt=""> | Isometric |

`PLAN` returns to the plan view of the current UCS. <img src="images/icons/persp.svg" width="20" alt=""> `PERSPECTIVE` switches between parallel and perspective projection.

## Visual styles

`VSCURRENT` (or the **Visual Styles** list on the View tab) sets how 3D objects are drawn:

| | Style |
|---|---|
| <img src="images/icons/wireframe.svg" width="20" alt=""> | Wireframe: edges only. |
| <img src="images/icons/hidden.svg" width="20" alt=""> | Hidden: edges, with hidden ones removed. |
| <img src="images/icons/solid.svg" width="20" alt=""> | Shaded, Realistic, Conceptual: shaded faces. |
| <img src="images/icons/xray.svg" width="20" alt=""> | X-ray: see-through faces. |

## Named views

<img src="images/icons/vports_named.svg" width="20" alt=""> `VIEW` (`V`) saves the current view under a name and restores it later. Named views are kept in the drawing, so the next person who opens it can use them too.

## Model viewports

`VPORTS` divides the drawing area into several viewports, each with its own view, for example a plan, a front view and an isometric side by side. The **Model Viewports** panel on the View tab has the common arrangements:

<img src="images/icons/vports_single.svg" width="20" alt=""> One · <img src="images/icons/vports_2v.svg" width="20" alt=""> Two vertical · <img src="images/icons/vports_2h.svg" width="20" alt=""> Two horizontal · <img src="images/icons/vports_3r.svg" width="20" alt=""> Three · <img src="images/icons/vports_4.svg" width="20" alt=""> Four

<img src="images/icons/tile_horiz.svg" width="20" alt=""> `HORIZONTAL` and <img src="images/icons/tile_vert.svg" width="20" alt=""> `VERTICAL` split the view into two viewports in one step.

Click in a viewport to make it current; **Ctrl+R** moves to the next one. <img src="images/icons/vports_join.svg" width="20" alt=""> `VPJOIN` goes back to a single viewport.

Viewports on sheets are a different thing; see [Layouts and Viewports](Layouts-and-Viewports).

## 3D mouse

On Windows, a 3D mouse (6-axis navigation device) works through its vendor driver. Choose its behaviour under **Options › User Preferences** or with `SPACEMOUSE`: follow the context, pan only, pan and zoom, or full 3D navigation.

## Several drawings

Each open drawing has a tab above the drawing area. **Ctrl+Tab** and **Ctrl+Shift+Tab** move between them.

## Redraw and regenerate

`REGEN` (`RE`) rebuilds the display of the current viewport from the drawing data, and `REGENALL` does it for all viewports. You rarely need them: the display updates on its own.
