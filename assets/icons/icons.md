# OpenCADStudio Icon Design Guidelines

## 1. Purpose

This document defines the visual language for toolbar, ribbon, command, and interface icons used in OpenCADStudio.

The goal is to create a calm, professional, and consistent icon system for CAD, BIM, drafting, modeling, and technical workflows. Icons must remain clear at small sizes, work across light and dark interface themes, and feel appropriate for engineering software.

The system should communicate precision, structure, and technical reliability without appearing visually heavy or overly decorative.

---

## 2. Design Direction

The preferred style can be described as:

> Minimal technical outline iconography for modern CAD software.

The icon system should use:

- Flat vector construction
- Thin and consistent outline strokes
- Simple geometric forms
- Restrained use of color
- Clear visual hierarchy
- Balanced spacing
- Minimal internal detail
- Strong readability at toolbar sizes
- A calm and professional appearance

The visual direction may feel familiar within modern professional design software, but icons should remain original and should not copy an existing proprietary icon set.

---

## 3. Core Visual Principles

### 3.1 Clarity

Each icon must communicate one primary action or concept.

Avoid adding secondary objects unless they are essential to understanding the command.

A user should be able to recognize the icon at 16 px, 20 px, and 24 px without needing a tooltip.

### 3.2 Simplicity

Reduce each concept to its most recognizable geometric representation.

Prefer:

- One boundary
- One primary object
- One secondary indicator
- One accent color

Avoid:

- Complex illustrations
- Decorative details
- Excessive control points
- Multiple competing symbols
- Small visual elements that disappear at low resolution

### 3.3 Consistency

All icons should feel as if they belong to the same interface.

Consistency must be maintained in:

- Stroke width
- Corner radius
- Object proportions
- Handle size
- Color usage
- Padding
- Visual weight
- Alignment
- Level of detail

### 3.4 Technical Precision

Shapes should look intentional and accurately constructed.

Use:

- Pixel-aware positioning
- Symmetrical geometry where appropriate
- Consistent spacing
- Clean path joins
- Rounded line caps only when they improve readability
- Sharp corners for explicitly technical geometry

### 3.5 Calm Visual Weight

The icon system should not feel loud, playful, glossy, or heavily saturated.

The overall appearance should remain quiet and suitable for long-term professional use.

---

## 4. Color Palette

### 4.1 Primary Neutral

Use the following neutral color for most outlines, boundaries, inactive handles, and internal geometry:

```text
#B4B6B9
```

Recommended use:

- Main icon outlines
- Geometric objects
- Selection boundaries
- Secondary indicators
- Inactive control points
- Supporting lines

### 4.2 Accent Blue

Use the following blue as the primary interaction and emphasis color:

```text
#6DB7ED
```

Recommended use:

- Active selection handle
- Primary command emphasis
- Selected element
- Action indicator
- Small focal detail
- Active state

### 4.3 Color Ratio

Most icons should use approximately:

- 80 to 90 percent neutral gray
- 10 to 20 percent accent blue

The blue should guide attention, not dominate the complete icon.

### 4.4 Additional Colors

Do not introduce extra colors unless the command requires a clear semantic meaning.

Examples:

- Red may indicate delete, remove, error, or destructive actions
- Green may indicate confirm, add, valid, or complete
- Orange may indicate warning or attention

Any additional semantic colors should follow the same restrained visual style.

---

## 5. Canvas and Sizing

### 5.1 Base Canvas

Design all icons using a standard SVG canvas:

```xml
viewBox="0 0 24 24"
```

The 24 by 24 canvas should be treated as the master grid.

### 5.2 Supported Display Sizes

Icons should remain readable at:

- 16 by 16 px
- 20 by 20 px
- 24 by 24 px
- 32 by 32 px
- 48 by 48 px for larger ribbon commands

The primary design target is 24 by 24 px.

### 5.3 Safe Area

Keep the primary artwork inside an approximate safe area:

```text
2 px to 22 px
```

Avoid placing important strokes directly against the canvas edge.

Use additional internal spacing when the icon contains:

- Selection handles
- Arrows
- External indicators
- Expansion symbols
- Rotation markers

---

## 6. Stroke Guidelines

### 6.1 Standard Stroke Width

Recommended default stroke width:

```text
1.4 to 1.6
```

Preferred starting value:

```text
1.5
```

### 6.2 Secondary Stroke Width

For less important internal details:

```text
1.0 to 1.25
```

Do not use very thin lines that disappear at 16 px.

### 6.3 Line Caps

Preferred values:

```xml
stroke-linecap="round"
stroke-linejoin="round"
```

Use square or mitered joins only when the command represents explicitly sharp construction geometry.

### 6.4 Stroke Consistency

Do not mix several unrelated stroke widths in one icon.

A typical icon should use:

- One primary stroke width
- Optionally one secondary stroke width
- No more than two visible stroke weights

---

## 7. Shape Language

### 7.1 Geometric Forms

Preferred base forms include:

- Rectangles
- Rounded rectangles
- Circles
- Arcs
- Triangles
- Straight technical lines
- Simple paths
- Minimal arrows
- Small control points

### 7.2 Corner Radius

Use restrained corner radii.

Recommended values on a 24 by 24 canvas:

```text
0.75 to 2.0
```

Use rounded corners for:

- Selection frames
- Panels
- Containers
- Generic grouped objects

Use sharper corners for:

- Drafting geometry
- Structural shapes
- Technical profiles
- Precise construction symbols

### 7.3 Selection Handles

Selection handles should be simple squares or circles.

Recommended size:

```text
2.0 to 3.0 units
```

Use the accent blue for the active handle.

Inactive handles may use the neutral gray or may be omitted when they add unnecessary noise.

---

## 8. Fill and Outline Usage

### 8.1 Default Treatment

The standard icon should primarily use outlines.

Recommended structure:

```xml
fill="none"
stroke="#B4B6B9"
```

### 8.2 Accent Fill

The accent color may use a solid fill for a small focal object:

```xml
fill="#6DB7ED"
```

Typical examples:

- Active handle
- Selected node
- Add indicator
- Primary object
- Active control point

### 8.3 Filled Objects

Use fully filled objects only when:

- The silhouette is more recognizable than an outline
- The object is very small
- The fill improves contrast
- The object represents an active state

Avoid filling every object in the icon.

---

## 9. Depth and Effects

The icon system must remain flat.

Do not use:

- Drop shadows
- Inner shadows
- Glow effects
- Bevels
- Embossing
- Metallic effects
- 3D rendering
- Lighting effects
- Blur
- Texture
- Photorealistic elements

Avoid gradients in standard toolbar icons.

A gradient may only be used for a special product illustration, not for routine command icons.

---

## 10. Background

All production icons should use a transparent background.

Do not include:

- White background rectangles
- Gray preview backgrounds
- Checkerboard patterns
- Toolbar panel colors
- Glow behind the icon

The interface should control the icon background.

Example SVG root:

```xml
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
```

No background element is required.

---

## 11. Visual Hierarchy

Every icon should contain a clear hierarchy.

### Primary element

The main command concept.

Examples:

- Group boundary
- Wall
- Line
- Circle
- Object
- Layer
- View
- Document

### Secondary element

The action or modifier.

Examples:

- Plus sign
- Minus sign
- Arrow
- Handle
- Lock
- Eye
- Cursor
- Link
- Break indicator

### Accent element

The small blue focal point.

Examples:

- Selected corner
- Active node
- Command result
- Main action handle

Avoid giving all elements equal visual importance.

---

## 12. Composition

### 12.1 Alignment

Align elements to a clear internal grid.

Preferred alignment:

- Centered
- Optically balanced
- Evenly spaced
- Consistent baseline
- Consistent object scale

### 12.2 Overlap

Small overlaps may help communicate relationships.

Examples:

- Grouped objects
- Linked objects
- Combined geometry
- Union
- Attachment
- Constraint

Avoid overlaps that make object boundaries difficult to read.

### 12.3 Empty Space

Use empty space intentionally.

A calm icon should not fill the entire canvas.

The space between elements should be large enough to remain visible at 16 px.

---

## 13. Icon States

### 13.1 Default State

Use:

- Neutral gray outlines
- Transparent background
- Minimal or no blue accent

### 13.2 Hover State

Possible treatments:

- Change the accent element to blue
- Increase contrast slightly
- Add a subtle blue fill to the primary indicator
- Keep geometry unchanged

Do not add glow or large background effects.

### 13.3 Active State

Use:

- Accent blue for the primary action
- Neutral gray for supporting geometry
- Stronger visual focus
- Same icon proportions

### 13.4 Disabled State

Use:

- Reduced opacity
- Neutral gray only
- No blue accent

Recommended opacity:

```text
35 to 50 percent
```

### 13.5 Destructive State

Use a restrained red only for commands such as:

- Delete
- Remove
- Break
- Explode
- Ungroup
- Clear

The rest of the geometry should remain neutral.

---

## 14. Group Command Icon Context

The Group command icon should communicate that multiple independent objects are treated as one logical unit.

### Recommended visual concept

Use:

- One shared outer selection boundary
- Two or three simple geometric objects inside
- One active corner handle in blue
- Neutral gray geometry
- No decorative connection line unless needed
- No large number of handles
- No multicolor internal shapes

### Preferred object combination

A recognizable set may include:

- Square
- Circle
- Triangle

These objects should be visually distinct but simple.

### Meaning

The outer frame communicates:

- Shared selection
- One grouped entity
- Common boundary
- Collective manipulation

The blue handle communicates:

- Active grouping
- Selection state
- Editable object control

### Avoid

Do not use:

- Four bright yellow corner handles
- Dashed boundaries with excessive detail
- Several unrelated colors
- Small diagonal connector lines
- Realistic object representations
- Heavy borders
- Glowing selection effects

---

## 15. SVG Construction Guidelines

### 15.1 Clean Source

SVG files should remain readable and easy to maintain.

Use:

- Clear indentation
- Logical element order
- Short comments
- Reusable color values where supported
- Minimal path complexity
- No unnecessary metadata

### 15.2 Example Structure

```xml
<svg
  xmlns="http://www.w3.org/2000/svg"
  viewBox="0 0 24 24"
  role="img"
  aria-label="Group"
>
  <rect
    x="3"
    y="4"
    width="18"
    height="16"
    rx="1.5"
    fill="none"
    stroke="#B4B6B9"
    stroke-width="1.5"
    stroke-linecap="round"
    stroke-linejoin="round"
  />

  <rect
    x="5.5"
    y="8"
    width="4.5"
    height="6"
    fill="none"
    stroke="#B4B6B9"
    stroke-width="1.5"
  />

  <circle
    cx="12.5"
    cy="11"
    r="3"
    fill="none"
    stroke="#B4B6B9"
    stroke-width="1.5"
  />

  <path
    d="M15.5 14.5L18 9.5L20.5 14.5Z"
    fill="none"
    stroke="#B4B6B9"
    stroke-width="1.5"
    stroke-linejoin="round"
  />

  <rect
    x="1.75"
    y="2.75"
    width="2.5"
    height="2.5"
    rx="0.5"
    fill="#6DB7ED"
  />
</svg>
```

### 15.3 Optimization

Before publishing:

- Remove unused groups
- Remove editor metadata
- Remove hidden layers
- Remove unnecessary transforms
- Avoid embedded raster images
- Avoid clipping paths unless essential
- Avoid masks unless essential
- Keep path coordinates understandable
- Verify that the SVG scales correctly

---

## 16. Theme Compatibility

Icons must work on both dark and light interface themes.

### Dark theme

The neutral gray `#B4B6B9` provides a soft contrast without appearing harsh.

The blue `#6DB7ED` remains visible and calm.

### Light theme

Test the icon against light gray and white interface backgrounds.

If `#B4B6B9` becomes too light on a specific surface, the interface may apply a darker theme token. Do not hardcode a dark background into the SVG.

### Recommended future approach

Where technically supported, map colors to interface variables:

```css
--icon-neutral: #B4B6B9;
--icon-accent: #6DB7ED;
```

This allows theme-specific adjustments while preserving the icon language.

---

## 17. Accessibility

Icons should not depend on color alone.

The shape and composition must remain understandable in grayscale.

Use color as a supporting signal, not as the only signal.

Ensure:

- Clear silhouettes
- Sufficient contrast
- Distinguishable action markers
- Consistent command placement
- Tooltips for all toolbar icons
- Accessible labels where SVGs are used directly

Example:

```xml
role="img"
aria-label="Group"
```

---

## 18. Naming and File Structure

Use lowercase file names with hyphens where needed.

Examples:

```text
group.svg
ungroup.svg
move.svg
copy.svg
rotate.svg
mirror.svg
trim.svg
extend.svg
layer-manager.svg
object-properties.svg
```

Avoid:

- Spaces
- Uppercase file names
- Version numbers in production file names
- Generic names such as `icon1.svg`

---

## 19. Review Checklist

Before approving an icon, verify the following:

- The concept is recognizable at 16 px
- The icon uses a 24 by 24 viewBox
- The background is transparent
- The stroke width is consistent
- The neutral color is `#B4B6B9`
- The accent color is `#6DB7ED`
- The blue accent is used sparingly
- No glow, shadow, gradient, bevel, or blur is present
- Internal details remain visible at small sizes
- The icon is visually balanced
- The icon matches the rest of the icon family
- The SVG contains no unnecessary metadata
- The icon works on both light and dark backgrounds
- The meaning remains understandable without color

---

## 20. Design Prompt Template

Use the following prompt when generating or briefing new icons:

> Create a minimal technical CAD toolbar icon in a clean flat vector outline style. Use a 24 by 24 canvas, thin consistent strokes, simple geometric forms, and balanced spacing. Use `#B4B6B9` for neutral outlines and `#6DB7ED` as a restrained accent color. Keep the background transparent. The icon must remain recognizable at 16 px and should contain no shadows, gradients, glow, texture, text, or unnecessary detail. The result should feel calm, precise, professional, and consistent with modern engineering software.

---

## 21. Summary

The OpenCADStudio icon system should be:

- Minimal
- Technical
- Precise
- Flat
- Quiet
- Consistent
- Scalable
- Theme-compatible
- Easy to understand
- Suitable for professional CAD and BIM workflows

The main visual identity is defined by neutral gray technical linework, a restrained light blue accent, simple geometry, and careful spacing.
