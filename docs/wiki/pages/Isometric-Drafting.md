# Isometric Drafting

Isometric drawings show three faces of an object in one 2D view, with the edges along three axes at 30°, 90° and 150°. They are drawn in 2D, with the help of an isometric crosshair and snap.

## Turning it on

`ISODRAFT` turns isometric drafting on or off. While it is on:

- the crosshair runs along the axes of the current **isoplane** instead of horizontal and vertical;
- Ortho (**F8**) and polar tracking keep lines along those axes.

Isometric drafting lasts for the session; it is off again the next time the program starts.

## Isoplanes

| Isoplane | Axes |
|---|---|
| Left | 90° and 150° |
| Top | 30° and 150° |
| Right | 30° and 90° |

Press **F5** (`ISOPLANE`) to move to the next isoplane, or type `ISOPLANE` and choose one. Choosing an isoplane also turns isometric drafting on.

## Drawing circles

A circle in an isometric view is an ellipse. Draw it with `ELLIPSE`, placing its axis ends along the isoplane's axes.

## Dimensioning

Use `DIMALIGNED` along the isometric axes, then `DIMEDIT` › **Oblique** to slant the extension lines to 30° or 150°, and a text style with a matching oblique angle.
