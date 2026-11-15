# STEP import fixtures

Read by the tests in `src/io/step_import.rs`.

Six files were written by FreeCAD 1.1.4 (Open CASCADE 7.8, AP214) with
`Import.export` from `Part::Feature` objects, each labelled as its product is
named, and each read back by FreeCAD as the shape it was given:

| File | Made with | What it holds |
| --- | --- | --- |
| `box.step` | `Part.makeBox(10, 20, 30)` | One product, `Box`, with one solid, in millimetres |
| `inch_box.step` | `Part.makeBox(1, 2, 3)` with `write.step.unit` = `INCH` | A 1 × 2 × 3 inch box: the unit is a conversion-based inch and the numbers are inches |
| `curved.step` | `makeSphere(5)`, `makeCylinder(3, 10)` at x = 20, `makeCone(4, 1, 8)` at x = 40, `makeTorus(6, 2)` at x = 60 | Four products, `Sphere`, `Cylinder`, `Cone` and `Torus`, under one root |
| `nurbs_sphere.step` | `Part.makeSphere(5).toNurbs()` | A sphere written as a rational B-spline surface (a complex entity) |
| `assembly.step` | An `App::Part` `Assembly` holding `Plate` (`makeBox(10, 20, 2)`) and `Post` (`makeBox(2, 2, 15)`, turned 90° about z and moved to x = 50) | Two components placed by their occurrences |
| `open_box.step` | `Part.Shell` of a 10 mm box's faces without the top | A shell-based surface model of five faces |

`write.step.unit` changes the unit the file names, not its numbers, which is
why the inch box is made 1 × 2 × 3.

Two files were derived from `box.step` by a script:

- `coloured_box.step` adds AP214 presentation styles: the solid in RGB (0.8,
  0.2, 0.1); its first face in the predefined colour `blue`; its second face
  green, overridden by yellow; its third face by a rendering style in RGB
  (0.25, 0.5, 0.75). FreeCAD still reads it as the same solid.
- `no_products.step` removes the product structure (every `PRODUCT*`,
  `APPLICATION_*`, `PRODUCT_DEFINITION_*`, `SHAPE_DEFINITION_REPRESENTATION`
  and `PRODUCT_RELATED_PRODUCT_CATEGORY` instance), leaving the
  representations, for the reader's path for files without products. Open
  CASCADE reads shapes only through products, so FreeCAD does not read this
  one.
