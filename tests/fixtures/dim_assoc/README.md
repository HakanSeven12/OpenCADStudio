# Viewport association fixture

`viewport_associations.dxf` contains line, circle, arc, polyline, spline,
block-instance, and intersection references through a 1:2 viewport.
`src/app/viewport_dimension_tests.rs` tests refresh and intersection source edits.

Each dimension stores `DIMLFAC=-2` and `ACAD_DIMASSOC_CALC_DIMLFAC=-2` so its paper
definition points measure in model units. The spline tangent from `(0, 300)`
touches `(90, 283.125)` at parameter `0.75`; tests verify its tangency and preserve
the dimension if an edited reference cannot resolve.
