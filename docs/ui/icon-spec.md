# Icon design spec (16 px and 32 px)

The rules for the tool icons in `assets/icons/`: how big things are, where
they sit on the pixel grid, which colours they use and what the SVG file may
contain. Every value below is taken from the icons already in the repository
and from the code that draws them, so a new icon drawn this way sits next to
the existing ones without standing out. The code that applies these rules is
`src/ui/icons.rs` (theme colours) and `src/ui/icons/grid.rs` (fitting to the
screen's scale).

## Where the files go

| Set | Folder | Used |
| --- | --- | --- |
| 16 px | `assets/icons/` and its subfolders (`arc/`, `circle/`, `model/`, …) | Below 24 logical px: the small ribbon, menus, palettes. |
| 32 px | `assets/icons/32/`, same relative path as its 16 px partner | From 24 logical px up (`ribbon_metrics::LARGE_ART_MIN`): the large ribbon buttons. |
| UI chrome | `assets/icons/ui/` (and `32/ui/`) | Carets, close, check, undo and the other window controls. |

A 32 px drawing always has a 16 px partner at the same path
(`every_large_icon_has_a_small_partner`). Partners are matched by content, so
two 16 px files must not be byte-identical when their 32 px drawings differ.
A 16 px icon without a 32 px drawing is fine; the 16 px art is then fitted up.

## Principles

1. **Same absolute stroke.** Every line is 1 px at both 16 and 32 px. A 32 px
   icon is not a 16 px icon scaled ×2: it gets more room for detail, not
   thicker lines.
2. **Redraw per size.** Draw the 16 px icon separately and simplify it. Never
   scale 32 down.
3. **Pixel-exact.** Every stroke and fill edge sits on a whole pixel: stroke
   centre lines on `.5`, fill edges on whole numbers.
4. **One unit = one pixel.** The `viewBox` equals the pixel size, with no
   transforms and no fractional offsets.
5. **Grips mean input.** Blue grips mark the points the user clicks to define
   the object, and only those.

## Size spec

All values are at 100 % scaling. Padding is measured to the outer edge of
the outermost element, grips included.

| Property | 16 px | 32 px |
| --- | --- | --- |
| Stroke (geometry, outlines) | 1 px | 1 px |
| Padding | 1 px (0 px when the shape needs it) | 2 px |
| Live area | 14 × 14 px | 28 × 28 px |
| Grip | 2 × 2 px filled | 4 × 4 px filled |
| Grip on a 1 px line's centre | 3 × 3 px filled | 5 × 5 px filled |
| Gap around a grip | 1 px (mask cut-out 4 × 4) | 1 px (mask cut-out 6 × 6) |
| Circle / arc | 14 px Ø (`r` 6.5 around 8, 8) | 28 px Ø (`r` 13.5 around 16, 16) |
| Isometric box | 13 × 14 px | 25 × 27 px |
| Arrowhead (filled triangle) | 5 px base, 3 px long | 9 px base, 6 px long |
| Min. gap between parallel lines | 1 px | 2 px |
| Grips per icon | usually 1–2 | usually 1–3 |

At 32 px the padding is 2 px; only a small detail such as an arrow tip may
enter it when legibility needs it. At 16 px, 1 px is the default, but a shape
that needs the full 16 px may run to the edge (0 px): overlapping circles
(`model/union`), a large arc (`rotate`) or a frame. About a third of the
16 px icons do. Use the room only when the shape needs it; an icon that fits
in 14 × 14 px keeps its 1 px. A circle fills the live area; a full square looks larger than a circle of the same width,
so it is drawn a little smaller.

## Pixel snapping

A centred 1 px stroke covers exactly one pixel only when its centre line is
on `x.5`; fills cover whole pixels when their edges are on whole numbers.

| Element | Rule | 16 px example | 32 px example |
| --- | --- | --- | --- |
| Horizontal / vertical stroke | Centre line on `.5` | `y = 7.5` | `y = 15.5` |
| Rectangle (stroked) | Corners on `.5` | `M1.5 2.5H14.5V13.5H1.5Z` | `M5.5 5.5H26.5V26.5H5.5Z` |
| Grip (even size) | Fill edges on whole numbers | `M1 13H3V15H1Z` | `M3 25H7V29H3Z` |
| Grip (odd size) | Centred on a `.5` line, edges whole | `M2 11H5V14H2Z` | `M13 2H18V7H13Z` |
| Circle / arc | Centre and radius land the stroke on `.5` | `A6.5 6.5` around 8, 8 | `A13.5 13.5` around 16, 16 |
| Diagonal | End points on pixel centres; slope 1:1 or 2:1 | `M1.5 14.5L14.5 1.5` | `M5.5 26.5L26.5 5.5` |
| Fill (face, arrowhead) | Edges on whole numbers, tips on the line they end | `M7.5 1L5 4L10 4Z` | `M15.5 2L11 8L20 8Z` |
| Stroke alignment | Always centred (SVG strokes are) | — | — |

Notes:

- Grips are drawn after the geometry. The geometry under and around a grip
  is cut away with a mask (`fill="#000"` square one pixel larger on every
  side), so a 1 px gap shows the background between the grip and the line.
- Diagonals and curves cannot be fully crisp, but with end points on pixel
  centres their anti-aliasing is symmetric. Isometric edges use a 2:1 slope
  so the staircase is regular.
- Draw diagonals in the same colour as straight lines. The renderer draws
  anti-aliased paths twice (`sharpen_text` in `icons.rs`), which brings their
  edge pixels up to the weight of a straight line, so no darker shade is
  needed at 16 px.

## Colour and style

Icons are drawn in fixed source colours that stand for a role, and
`recolor_semantic_svg` (in `icons.rs`) swaps each one for the matching
colour of the active theme when the icon is drawn. Use only these source
colours, written exactly like this (lower case, 6 digits):

| Role | Source colour | Studio Dark | Light | Use |
| --- | --- | --- | --- | --- |
| Geometry | `#ffffff` | `#dfe3e9` (theme text) | `#000000` | Lines, arcs, edges, outlines, neutral arrows |
| Interaction | `#4a9eff` | `#3ea6ff` (accent) | `#5865f2` | Grips, the active element, direction of the action |
| Annotation | `#f0c040` | `#f5c84b` (warning) | `#b1782c` | Dimensions, leaders, measuring |
| Remove / error | `#e05050` | `#f2645f` (danger) | `#c3423f` | Delete, remove, cancel |
| OK / current | `#4ccf6f` | `#3ecf6e` (success) | `#12664f` | Confirm, set current, unlock |
| Neutral face | `#ffffff` with `fill-opacity` 0.05–0.2 | text at that opacity over the panel | — | The sides of a solid |
| Active face | `#4a9eff` with `fill-opacity` 0.15–0.4 | accent at that opacity over the panel | — | Planes, the face being extruded |
| Grip gap | transparent (mask cut-out) | the panel behind it, `#3c4451` | the panel | Around every grip |

The Studio Dark column is what `SemanticColors::from_theme` gives for the
Studio Dark theme (`studio_theme.rs`), the design theme the chrome colours in
`style/chrome.rs` are tuned on. The Light column is iced's Light theme. Every
other theme gets its own values the same way: geometry is the theme's text
colour, and the accents keep their hue but are pushed towards the text until
they reach 3:1 on the ribbon panel (`readable::icon_accent`). So an icon has
no dark-mode variant of its own: the same file serves every theme, and the
colours are the same at 16 and 32 px.

`recolor_semantic_svg` also maps a few older source colours (for example
`#888888`, `#1565c0`, `#e1e1e1`); see `semantic_color` for the full list.
Any colour that is not in that list is drawn as written in every theme, so a
dark grey like `#444444` would disappear on a dark theme. Mask colours are
the exception that must not follow the theme: write them as the 3-digit
`#fff` and `#000`, which are not recoloured.

- **Arrows.** The direction of the action is blue (extrude). Arrows
  of a dimension are yellow like the rest of the annotation. Arrows that are
  the tool itself (move, pan) are drawn in the geometry colour.
- **One projection.** 2D tools are drawn flat; 3D tools use one isometric
  angle (2:1 pixel slope) at both sizes, planes included.
- **Caps and joins:** butt caps and miter joins, set once on the root
  `<svg>`; no rounded corners. (The 16 px `civil/` set still uses round caps
  in places; do not copy that.)
- **Detail by size:** extra edges, hidden faces, dashed construction lines
  or a third grip only at 32 px.

## Reference icons

The line and rectangle tools use grips at the outer limit, so the padding is
exactly 1 px at 16 and 2 to 3 px at 32.

| Icon | Element | 16 px | 32 px |
| --- | --- | --- | --- |
| `line` | Line | `M1.5 14.5L14.5 1.5` | `M5.5 26.5L26.5 5.5` |
| `line` | Grips | `M1 13H3V15H1Z`, `M13 1H15V3H13Z` | `M3 25H7V29H3Z`, `M25 3H29V7H25Z` |
| `line` | Mask cut-outs | `M0 12H4V16H0Z`, `M12 0H16V4H12Z` | `M2 24H8V30H2Z`, `M24 2H30V8H24Z` |
| `shapes/rect` | Rectangle | `M1.5 2.5H14.5V13.5H1.5Z` | `M5.5 5.5H26.5V26.5H5.5Z` |
| `circle/circle_2p` | Circle | `r` 6.5 around 8, 8 | `r` 13.5 around 16, 16 |
| `model/box` | Isometric box | `M7.5 1.5L13.5 4.5L13.5 11.5L7.5 14.5L1.5 11.5L1.5 4.5Z` | `M15.5 2.5L27.5 8.5L27.5 22.5L15.5 28.5L3.5 22.5L3.5 8.5Z` |
| `move` | Arrowhead | `M7.5 1L5 4L10 4Z` | `M15.5 2L11 8L20 8Z` |

Line tool, 16 px (`assets/icons/line.svg`):

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" stroke-width="1" stroke-linecap="butt" stroke-linejoin="miter">
  <defs>
    <mask id="m1" maskUnits="userSpaceOnUse" x="0" y="0" width="16" height="16">
      <path d="M0 0H16V16H0Z" fill="#fff"/>
      <path d="M0 12H4V16H0ZM12 0H16V4H12Z" fill="#000"/>
    </mask>
  </defs>
  <g mask="url(#m1)"><path d="M1.5 14.5L14.5 1.5" fill="none" stroke="#ffffff"/></g>
  <path d="M1 13H3V15H1Z" fill="#4a9eff"/>
  <path d="M13 1H15V3H13Z" fill="#4a9eff"/>
</svg>
```

Line tool, 32 px (`assets/icons/32/line.svg`):

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32" stroke-width="1" stroke-linecap="butt" stroke-linejoin="miter">
  <defs>
    <mask id="m1" maskUnits="userSpaceOnUse" x="0" y="0" width="32" height="32">
      <path d="M0 0H32V32H0Z" fill="#fff"/>
      <path d="M2 24H8V30H2ZM24 2H30V8H24Z" fill="#000"/>
    </mask>
  </defs>
  <g mask="url(#m1)"><path d="M5.5 26.5L26.5 5.5" fill="none" stroke="#ffffff"/></g>
  <path d="M3 25H7V29H3Z" fill="#4a9eff"/>
  <path d="M25 3H29V7H25Z" fill="#4a9eff"/>
</svg>
```

(The files in the repository are one line each; they are broken up here to
read.)

## File format

The fitting in `grid.rs` reads the SVG itself, so the file has to stay
within what it understands. The tests `icon_files_are_square_pixel_grid_svgs`
and `every_icon_fits_every_scale_and_still_parses` fail on an icon that does
not.

- The root tag is `<svg xmlns=… width="16" height="16" viewBox="0 0 16 16"
  stroke-width="1" stroke-linecap="butt" stroke-linejoin="miter">` (or 32),
  with the `viewBox` written exactly like that: it is how the art size is
  recognised.
- Only `<path>`, `<g>`, `<defs>`, `<mask>` and `<clipPath>`. No `<rect>`,
  `<circle>`, `<line>`, `<polyline>` or `<text>`: draw them as paths.
- No `transform`, `<style>`, `class=`, `currentColor` or `var(`.
- A stroked path has `fill="none"` and a `stroke`; a filled path has no
  stroke. Set the line width once on the root, not per path: the fitting
  changes the root's `stroke-width` for the screen's scale, and a path with
  its own width would keep it.
- A grip is a filled axis-aligned square (`M x y H … V … H … Z`) outside any
  mask, no larger than a fifth of the art (3.2 px at 16, 6.4 px at 32). Only
  then is it recognised as a grip and kept at one size at every scale.
- A mask cut-out is a `fill="#000"` square centred on its grip, at most 2 px
  larger on each side. Masks use `maskUnits="userSpaceOnUse"` with the full
  art as their region.
- Dashes (`stroke-dasharray`) are in whole pixels; they are scaled with the
  art.
- When optimising with SVGO, keep at least one decimal, so `.5` values are
  never rounded.

## Scales other than 100 %

Each icon ships as two SVGs, 16 px and 32 px, and nothing else: no PNGs and
no @2x files. At 125 %, 150 % and 175 % the screen's pixels no longer line up
with the art's, so `grid::fit` redraws the art for the exact physical size,
the way font hinting does:

| Scale | 16 px art drawn | Line weight |
| --- | --- | --- |
| 100 % | 16 px | 1 px |
| 125 % | 20 px | 1 px |
| 150 % | 24 px | 2 px |
| 175 % | 28 px | 2 px |
| 200 % | 32 px | 2 px |

From 150 % every line is 2 px, on purpose: a 1.5 px line cannot be drawn
sharp, so the weight is rounded (`grid::line_weight`). Lines are never thicker than the
ribbon's at the screen's scale, so an icon in a 20 or 22 px slot does not
look heavier than the ribbon next to it. Grips keep one size everywhere
(`round(w × scale)`) and their gaps stay even on every side. The 32 px art
follows the same rules from 24 logical px up.

Turn on the pixel grid and snap to pixels in the drawing tool. In Inkscape,
set the document scale to 1 user unit = 1 px. Check every icon at 100 %
zoom, and also at 150 %, where the 2 px lines show any edge that is off the
grid.

## Checklist per icon

- [ ] `viewBox` equals the pixel size, root tag as above, no transforms
- [ ] Only `<path>`, `<g>`, `<defs>`, `<mask>`, `<clipPath>`
- [ ] All strokes 1 px (from the root), centred, centre lines on `.5`
- [ ] Fills on whole numbers
- [ ] Outer edge of every element inside the live area: 2 px padding at
      32; 1 px at 16, or 0 px only when the shape needs the full size
- [ ] Grips 2 × 2 / 4 × 4 (or 3 × 3 / 5 × 5 on a line's centre), only on
      input points, drawn last, with a 1 px mask gap
- [ ] Only the source colours above; masks in `#fff` / `#000`
- [ ] Geometry `#ffffff`, grips and the action blue, annotation yellow
- [ ] Same isometric angle as the other 3D icons
- [ ] 16 px version redrawn, not scaled; the 32 px file at the same path
      under `32/`
- [ ] 16 px and 32 px side by side: line weight looks equal
- [ ] `cargo test icons` passes
