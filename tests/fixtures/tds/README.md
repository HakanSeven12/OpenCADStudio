# 3D Studio import fixtures

Read by the tests in `src/io/tds_import.rs`.

Written by Blender 5.2.3 with the "Autodesk 3D Studio (.3ds)" extension
3.0.2, exporting with z up and y forward, transforms applied and a scale of
1, and read back by the same extension:

| File | What it holds |
| --- | --- |
| `box.3ds` | `Box`, a 2 × 3 × 4 box centred on (10, 20, 30): 14 vertices (the texture seams split some corners), 12 triangles, all in material `Red` (diffuse 204, 25, 25; specular 255, 255, 255; shininess 60 %; transparency 25 %; Phong shading) |
| `two_materials.3ds` | `Crate`, a 2 m cube standing on the origin, whose top and bottom (4 triangles) are `Blue` and whose sides (8 triangles) are `Green` |
| `lights_camera.3ds` | `Floor` (a 10 × 10 × 0.2 slab below z = 0, material `Grey`), `Lamp` (a point light at (4, −3, 5), colour (1, 0.5, 0.25)), `Spot` (a spotlight at (0, 0, 8), colour (0.25, 0.5, 1), hotspot 45°, falloff 60°) and `Camera` (at (0, −20, 10), 35 mm lens), written with the keyframer section |
| `textured.3ds` | `Tile`, a unit cube whose material `Checker` has the diffuse map `checker.png` at 100 %; the image is not included |
| `objects.3ds` | Four unit cubes named `zeta`, `Alpha`, `mid/part` and `Beta`, in that order, the first three with their own material and `Beta` with Blender's default material `None` |

Blender writes the spotlight's target as (0, 0, −8) and the camera's as
(0, 20, −20); the tests take them as the file holds them. Blender's own
import of `lights_camera.3ds` reads `Floor` 0.1 too high unless it also
imports the keyframes, and gives the camera no aim; the file's vertices
and camera chunk are as listed above.
