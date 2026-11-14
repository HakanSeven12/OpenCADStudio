# UI kit — next steps: menus, dropdowns, ribbon

Follows `PALETTE_FAMILY_PLAN.md` (done: every palette on `ui::kit`). This
plan takes the kit to the parts of the UI that still build their own
menus, dropdown lists and controls. Counts are from a scan of the source
(tests excluded) on `ui/text-pallets-documenttabs-redesign`.

## 1. Where things stand

**Palettes.** On the kit with no generic colours or literal sizes:
Browser, Point Cloud Manager, Tool Palettes, Authoring Palettes,
Parameters Manager, Layer Walk, Layers, External References, plugin
panels, Assistant. Left:

| File | colours / literals / radii | Where |
|---|---|---|
| `window/sheet_set.rs` | 2 / 124 / 2 | the inline dialogs (form, wizard, properties, block list, template, category) |
| `window/quick_calc/*` | 6 / 91 / 20 | units and mode switch, variables form, keypad memory list and flyouts; only `view.rs` is audited |
| `properties.rs` | 54 / 25 / 8 | outside the grid: hatch pattern picker, parameter rows, editable choices, entity links, bool rows, input styles |
| `node_graph.rs` | 11 / 25 / 2 | the nodes on the canvas (the panel is done) |
| `window/count_palette.rs` | 3 / 17 / 1 | the invalid-area notice, the toolbar |
| `app/dock/view.rs` | 22 / 22 / 8 | the floating title bar's menu, drag ghosts and drop marks |

**Menus.** About ten implementations, each with its own row height,
padding, check position and frame:

| Menu | File | Row |
|---|---|---|
| palette menu (the kit's) | `ui/palette_menu.rs` | 22 |
| ribbon right-click menu | `ui/ribbon/ribbon_menu.rs` | own, check on the right, 12 px text, buttons |
| application menu | `ui/ribbon/app_menu.rs` | 28 |
| viewport context menu | `ui/popup/context_menu.rs` | 22, own code |
| menu bar | `ui/menu_bar.rs` | 25 |
| MText menu | `app/view/overlay/mtext_menu.rs` | 24 |
| status bar menu, viewport control menus | `ui/statusbar/statusbar_menu.rs`, `app/view/controls.rs` | own |
| floating title bar menu | `app/dock/view.rs` | generic colours, rounded |
| Layers tree and row menus, Xref row menu and flyout, Sheet Set menus | the palettes | tokens, but each built by hand |

**Dropdown lists.** The ribbon builds its own lists (layer list with
icons, annotation layer, linetype, lineweight, style, colour:
`ribbon/mod.rs`, `ribbon/widgets.rs`, `ribbon/color_dropdown.rs`).
Properties has three kinds (iced combo boxes, `DropdownList` in an
iced_aw drop-down, `wide_menu`). The colour picker (`color_select`), used
by Properties, Layers and the ribbon, still has `popup_panel_style` with
generic colours.

**Ribbon.** Tokenized (`ribbon_metrics`, `chrome`, hairlines) but no kit
at all: its own tooltip helpers (`make_tip`/`tip_style`, ~30 uses, the
style `kit::tooltip` copies), `tool_btn_style` (the same idea as
`kit::flat_button_style`), its own search and text boxes, and literal
sizes in `large_item`, `draw_panel` and `visibility_panel`. Panel frames,
the title strip, key tips, collapsing and floating panels stay the
ribbon's own.

## 2. Prompts

One PR each, in this order. Each ends with the family screenshots and the
palette audit.

| # | Prompt | Files | Size |
|---|---|---|---|
| 1a | **One menu, palettes first.** `palette_menu` gains shortcuts (muted, right-aligned), submenus (a flyout beside the row, opened on hover, the menu keeping one width) and an `on_enter` for closing a flyout. The floating title bar menu, Layers' tree and row menus, the Xref row menu and Sheet Set's menus move onto it. | `palette_menu.rs`, `app/dock/view.rs`, `layers.rs`, `xref_manager.rs`, `sheet_set.rs` | M |
| 1b | **Drawing menus.** The viewport context menu, status bar menu, viewport control menus and MText menu move onto `palette_menu`. | `popup/context_menu.rs`, `statusbar/statusbar_menu.rs`, `app/view/controls.rs`, `mtext_menu.rs` | L |
| 1c | **Ribbon menus.** The ribbon right-click menu, application menu and menu bar move onto `palette_menu` (rows become `mouse_area`s, as every context menu needs). | `ribbon/ribbon_menu.rs`, `ribbon/app_menu.rs`, `menu_bar.rs` | M |
| 2a | **`kit::dropdown`.** A field-style head and a list under it from its left edge, at least the field's width and as wide as its longest option, ellipsis, keys, an optional icon or swatch per row, in `pick_menu_style`. Properties' choice rows, block picker, editable choices and lineweight move onto it. | `kit/dropdown.rs`, `properties.rs` | L |
| 2b | **Layers and the colour picker** on `kit::dropdown`; `popup_panel_style` goes. | `layers.rs`, `color_select/*` | M |
| 2c | **Ribbon lists** (layer, annotation layer, linetype, lineweight, style, colour) on `kit::dropdown`. | `ribbon/mod.rs`, `ribbon/widgets.rs`, `ribbon/color_dropdown.rs` | L |
| 3 | **Ribbon tooltips and buttons** through `kit::tooltip` and one flat button style (size as a parameter); literal sizes to `ribbon_metrics`. | `ribbon/*` | S |
| 4 | **Palette remnants.** Sheet Set's dialogs, QuickCalc (and its two view files into the audit), Properties outside the grid, Count, the dock's ghosts and marks, Node Graph's nodes. | as listed in section 1 | M |
| 5 | **The rest.** The command line bar, the Start page. The dialogs are not part of this plan. | `command_line.rs`, `app/view/mod.rs` | M–L |

**Status.** 1a–5 are done. The command line bar and the Start page take
their colours from `chrome`; the Start page keeps a larger scale of its
own (`style::start_metrics`). Left as they are: the lineweight and
linetype rows stay searchable combo boxes in the kit's list style;
QuickCalc keeps its own derived key colours. The dialogs are out of this
plan's scope.

## 3. Rules

As in `PALETTE_FAMILY_PLAN.md`: behaviour unchanged (messages, keys,
persisted state); colours from `chrome::`, text from `text::`, sizes from
`palette_metrics` / `ribbon_metrics`; drawn lines `palette_metrics::line()`;
format only touched hunks; no new clippy warnings in touched code.
