# MTEXT editor

This note describes the in-place MTEXT editor as it stands after the October 2026 series of pull requests (listed at the end), what it guarantees about the drawing's data, and what is still open.

## Goal

The editor used to rebuild the whole MTEXT value from its own document model on every change. The first keystroke could therefore change tab stops, extended `\p` paragraph settings, nested groups, font flags or any other code the model did not keep exactly. Saving also replaced fields of the entity it did not show.

The rewrite keeps one rule throughout: **the MTEXT source is the only editable document.** Every command patches the bytes it changes and nothing else, and a patch is kept only when the result is exactly what was asked for. When that cannot be proved, the command is refused with a message and the text, the caret and the undo history stay as they were.

## How an edit works

The editor holds the raw value (`content`). The preview, the caret positions and the character boxes are derived from it on every change (`rebuild_mtext_preview`); they are never edited themselves.

| Module (`src/app/mtext_editor/`) | Role |
|---|---|
| `raw_adapter.rs` | `SourceIndex`: a lossless tokenisation of the source into controls, groups, characters, breaks and stacks, with the visible-character position of each. It decides which sources can be edited safely (unknown or incomplete controls, nesting limits, protected field expressions). |
| `raw_operations.rs` | The patches: replace a visible range, format a range (codes shared by adjacent units, an enclosing group updated in place so repeated toggling never nests deeper), paragraph settings that follow a split or a merge, and `certify`, which re-parses the result and checks the visible text. |
| `raw_edit.rs` | `edit_raw`, the single transaction every text command goes through; typing, delete, paste, `%%d/p/c`; bold, italic, underline, overline, strike and case; the format-button state and the format for the next typed text. |
| `raw_clear.rs` | Clear formatting: removes or splits only the codes that format the selection; the object font and width stay. |
| `raw_span.rs` | Values for a selection (height, oblique, width factor, tracking, font, colour), paragraph alignment, indents and spacing, and replace-all. Each patch is certified to change exactly that value. |
| `raw_stack.rs` | Superscript, subscript, Stack and the Fraction panel, all as one standard `\S` stack. |
| `raw_list.rs` | Bullets and numbering as the format stores them (a hanging indent plus marker and tab), Enter in a list item. |
| `raw_style.rs` | Object values: the object font and width are the `\f`/`\W` that open the source; text style changes keep the width the text draws with. |
| `raw_geometry.rs` | Object rotation. |
| `entity_options.rs` | Background mask and drawing direction, stored on the entity. |
| `raw_field.rs` | Text around native FIELDs. |
| `raw_commit.rs` | Apply / Save & Close, undo and redo. |

Rules every patch follows:

- Only the bytes of the edited place change; every other control, group and character stays byte for byte.
- A stack (fraction, index) is one atom: it is created, replaced or removed whole, and the caret steps over it.
- Formatting rules are the editor's own: italic is the 15° oblique (`\Q15;`), bold names the face, and both work on stroke fonts as before.
- The `^` of a stack is written without a following space, so it reads back byte for byte from both DWG and DXF.

## The window

- **First row: the object's own values**, each labelled: Style, Font, Height, Width factor, Rotation, Color, Annotative. Font and Width factor show where their value comes from: *By style*, *Custom*, or *Mixed* when characters override the style. With a selection, Font, Height, Width factor and Color format the selection instead.
- **Second row:** B, I, U, overline, strike (highlighted when on, a warning border when mixed, inert when the selection cannot be formatted; with nothing selected they set the format of the next typed text), undo and redo, paragraph alignment, and the menus. Below its full width the row scrolls sideways.
- **Menus:** More formatting (superscript, subscript, Stack, Fraction…, oblique angle, tracking, UPPERCASE / lowercase, clear formatting), Paragraph (lists, tab, line spacing factor and mode, indents and spacing), Columns, Justification, Position (drawing direction), Background (mask), Symbols, Find and replace. An action closes its menu; typing in a menu field keeps it open; Escape closes the open menu before the editor.
- **Preview:** the text as it will draw, at its rotation, with a box-width ruler above it (the box grows from its attachment point) and a full-line selection band.
- **Status bar:** character and paragraph counts, the box width, and Cancel (discard, as Escape), Apply (save and keep editing) and Close Text Editor (save and close).
- The window does not shrink below 900 × 370 px, although a smaller window still caps it.

## Saving and undo

Apply and Save & Close share one transaction (`commit_mtext_draft`):

- An invalid height, width factor, rotation or mask factor stops the save with a message; the draft stays open.
- An existing text is not emptied here (the drawing's delete is for that); an empty new text is dropped on close.
- A locked layer, a deleted target or a failed creation keeps the draft and says why.
- An unchanged save changes nothing: no drawing undo step and no dirty flag.
- Annotation scales change only when the annotative flag itself changes.
- The entity's other data is kept: XData, extension dictionary, groups, annotation scales, block ownership, a frame bit or mask transparency the editor does not show, and manual column heights while the column settings are unchanged.

Undo and redo restore every value of a step, not only the text: style, font, width, rotation, colour, height, line spacing and its mode, mask and direction drafts, box width, the selection and the pending format.

## Native FIELDs

On opening an MTEXT that hosts fields, the editor locates each field's text in the value with the codec's `field_spans`. Those spans are atomic: typing into, deleting or formatting over them is refused. Every other edit is allowed, and the spans move with each edit; where equal characters make the place ambiguous, the caret decides.

On save only the host's `_text` container changes. It gets the new template (each field's text back to its `%<\_FldIdx n>%`) and the new display checksum; every FIELD object stays as it is. A host whose fields cannot be located stays read-only.

## Saving to older formats

Saving to a DWG or DXF version that cannot hold an MTEXT feature (columns, a defined box height, a background mask, line spacing) asks first and names what will be lost; DXF before R2007 also loses columns. Headless saves and exports need `--allow-lossy` (exit code 3 otherwise). This comes with #2380 and #2381; see `MTextTargetLoss` in `src/io/mod.rs`.

## Open items

- **Rotation on a tilted text plane.** The codec writes the DWG x direction as `(cos r, sin r, 0)`, which leaves the text plane for any normal but Z. The editor therefore offers rotation on the XY plane only. The ignored test `rotation_tests::tilted_plane_text_keeps_its_in_plane_x_direction_through_dwg` describes the needed codec change: write and read the in-plane direction from the arbitrary-axis OCS axes.
- **Bold and italic on stroke fonts.** The format stores bold and italic only for TrueType faces; this editor keeps its own rule (bold names the face, italic is the oblique) so stroke-font text behaves as before. Adopting the format's rule would be a separate decision.
- **Style, Font and Color lists** are the standard drop-down lists; a font the text uses but the machine lacks is shown but not marked as missing.

## Tests

Each module has its own tests next to it (`*_tests.rs` in `src/app/mtext_editor/` and `src/app/view/overlay/mtext_tests.rs`). The window is exercised with the `iced_test` simulator: every menu command, popup placement, Escape order, the ruler and the status bar. `data_tests.rs` checks that an MTEXT's relations survive unchanged, property and text commits.

## The pull requests

| PR | Change |
|---|---|
| [#2372](https://github.com/HakanSeven12/OpenCADStudio/pull/2372) | Mask and frame drawn on text with run-less strokes |
| [#2373](https://github.com/HakanSeven12/OpenCADStudio/pull/2373) | Horizontal fractions centred |
| [#2374](https://github.com/HakanSeven12/OpenCADStudio/pull/2374) | Caret boxes for tabs and vertical text |
| [#2380](https://github.com/HakanSeven12/OpenCADStudio/pull/2380) | Warning before saving MTEXT features to a format that drops them |
| [#2381](https://github.com/HakanSeven12/OpenCADStudio/pull/2381) | The same for automation, CLI and control saves |
| [#2382](https://github.com/HakanSeven12/OpenCADStudio/pull/2382) | Lossless index of the raw source |
| [#2383](https://github.com/HakanSeven12/OpenCADStudio/pull/2383) | Certified source patches |
| [#2385](https://github.com/HakanSeven12/OpenCADStudio/pull/2385) | Typing, delete and paste on the source |
| [#2390](https://github.com/HakanSeven12/OpenCADStudio/pull/2390) | B, I, U, overline, S and case on the source |
| [#2392](https://github.com/HakanSeven12/OpenCADStudio/pull/2392) | Clear formatting |
| [#2393](https://github.com/HakanSeven12/OpenCADStudio/pull/2393) | Object font and width, labelled first row |
| [#2394](https://github.com/HakanSeven12/OpenCADStudio/pull/2394) | Rotation |
| [#2395](https://github.com/HakanSeven12/OpenCADStudio/pull/2395) | One save path, full undo, object colour |
| [#2396](https://github.com/HakanSeven12/OpenCADStudio/pull/2396) | Two-row toolbar with menus, minimum size |
| [#2397](https://github.com/HakanSeven12/OpenCADStudio/pull/2397) | Format-button state, format for the next typed text |
| [#2398](https://github.com/HakanSeven12/OpenCADStudio/pull/2398) | Selection values, alignment, indents, replace-all |
| [#2399](https://github.com/HakanSeven12/OpenCADStudio/pull/2399) | Box-width ruler, selection band, status bar |
| [#2400](https://github.com/HakanSeven12/OpenCADStudio/pull/2400) | Superscript, subscript, Stack, fractions |
| [#2402](https://github.com/HakanSeven12/OpenCADStudio/pull/2402) | Lists, tab, line spacing mode |
| [#2404](https://github.com/HakanSeven12/OpenCADStudio/pull/2404) | Background mask and drawing direction |
| [#2406](https://github.com/HakanSeven12/OpenCADStudio/pull/2406) | Text around native FIELDs, rotation acceptance test |
| this one | Clean-up and this note |

#2372–#2381 are independent of each other except #2381 (on #2380). From #2382 on, each pull request builds on the one before it.
