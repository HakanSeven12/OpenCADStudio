# Drawing Setup and Units

A drawing's geometry is just numbers. Three separate settings decide what those numbers mean and how they are shown, and it helps to keep them apart:

| Question | Setting | Changes the geometry? |
|---|---|---|
| How are lengths and angles **written**? | `LUNITS`, `LUPREC`, `AUNITS`, `AUPREC` | No |
| What does **one unit measure** (a millimetre, an inch, a metre)? | `INSUNITS` | No |
| **Convert** the drawing to another unit | `DWGUNITS` | Yes |

## The Units dialog

`UNITS` (`UN`) opens the drawing units. The same choices are on the status bar's units button.

**Length type** (`LUNITS`):

| Type | Example |
|---|---|
| Decimal | 33.5000 |
| Architectural | 2'9 1/2" |
| Engineering | 2'-9.5000" |
| Fractional | 33 1/2 |
| Scientific | 3.3500E+01 |

**Angle type** (`AUNITS`): decimal degrees (45.0000), degrees/minutes/seconds (45d0'0"), grads (50.0000g), radians (0.7854r) or surveyor's units (N 45d0'0" E). Each format carries its own mark, so a written angle says which kind it is.

**Precision** (`LUPREC`, `AUPREC`) sets how many decimals (or how fine a fraction) are shown.

**Angle direction.** `ANGBASE` sets where 0° points (east by default) and `ANGDIR` whether angles count counter-clockwise (default) or clockwise. Survey drawings often use north as 0° and clockwise angles.

Whatever format is set, you can type values in it: `5'-6 1/2"` in architectural units, `N45d20'6"E` as a bearing.

## Insertion units

`INSUNITS` labels what one drawing unit measures. Nothing moves when you change it. Its job is to tell other drawings how to read this one: when a block, drawing or reference is inserted, its units and the target's units decide the scale, so a detail drawn in millimetres lands at the right size in a plan drawn in metres.

## Converting a drawing

`DWGUNITS` converts the drawing to another unit: it changes the unit label **and** scales the geometry, so a drawing in inches becomes the same drawing in millimetres. Use it when you really want the numbers to change.

## Limits

`LIMITS` sets a rectangular drawing area. The grid shows only inside it, and with `LIMCHECK` on you cannot pick points outside it. `ZOOM` › **All** shows the limits or the extents, whichever is larger.

## Starting from a template

A template (`.dwt`) is a drawing kept as a starting point: units, layers, text and dimension styles, layouts and title blocks already set. `QNEW` (**Ctrl+N**) uses the default template from **Options › Files**; `NEW` asks which one. Save any drawing as `.dwt` to make your own.

## Drawing properties

`DWGPROPS` records a title, subject, author, keywords and your own properties in the drawing, and shows statistics such as when it was created and how long it has been edited. Fields in title blocks can show these values.

## Geographic location

When a drawing carries geographic data (a coordinate system and a location on the earth), the status bar shows the coordinate system's name; hover it for the details. Georeferenced images (with geo tags or a world file) can be attached at their own location; see [References and Underlays](References-and-Underlays).
