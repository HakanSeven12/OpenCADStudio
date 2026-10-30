# LAYWALK and the LayerWalk pallet

`LAYWALK` opens a dockable, non-modal pallet listing the drawing's layers. Click
a row and only the objects on the selected layers stay on screen; everything
else is temporarily hidden. The name, the selection/hold semantics and the
context menu follow the familiar Layer Walk command; the panel itself is built
on this app's dock infrastructure, not on a modal dialog.

The ribbon entry lives in the Home tab's expanded Layers panel and the menu
entry under **Format ▸ Layer Walk**; both are labelled *Layer Walk*, while the
pallet title is `LayerWalk - Layers: N`. There is no `-LAYWALK` command-line
variant and no Help button.

## Visibility mechanism: a scene-side override

While walking, nothing is written to the layer table. The walk installs a set of
*visible layer handles* on the space being interacted with
(`Scene::set_space_layer_walk`) and every visibility gate — render, hit-test,
selection, snapping, regen and the plotting path — ANDs in
`Scene::layer_walk_hidden()` next to its existing off/frozen test.

This is the mechanism the spec asked for and the renderer supports:

- the drawing is not marked dirty and undo records nothing while walking, so
  *Restore on exit* (the default) returns the exact drawing the user started
  from;
- a hidden layer stays selectable-proof: it cannot be picked, snapped or
  highlighted, which is what "temporarily hide" is supposed to mean;
- the override is per space. Model space, a layout sheet and an entered model
  viewport each have their own key, and only one space is ever walked — the
  pallet follows the user between them instead of hiding a drawing they left;
- a walk that shows every layer installs no override at all, so a freshly
  opened pallet is a genuine no-op rather than a full-size set.

Committing is the separate, explicit act: **Keep Result**, or closing with
*Restore on exit* unticked, writes the walked visibility into the layer table
as exactly one undo step, mirroring LAYOFF/LAYON (a layer the walk shows is
thawed as well as turned on; the *Layer 0 always on* pin is the only thing that
keeps a layer out of the commit).

Every path that ends a walk tears the override down — Close, the dock chrome,
the `LAYWALK` toggle, `reset_layer_walk_for` on a document load, and any
command that takes ownership of layer visibility (UNDO, LAYOFF, LAYISO, LAYMRG,
PURGE, VPLAYER, ERASE, NEW/OPEN/SAVEAS …). A transparent command such as
`'ZOOM` deliberately does not. A drawing is therefore never left half-hidden.

## The list

Rows carry the colour swatch, name, object count, the Always-Show pin and one
eye for the state the walk actually produces, and sort by name or count; frozen
and locked stay as small marks behind the eye. The eye reads open only when the
drawing really shows the layer, so a row the walk hides and a row that is off or
frozen in the table both read closed, and the tooltip says which. On open, every
layer the drawing itself shows is selected. A click replaces the selection,
Ctrl+click toggles, Shift+click and click-drag take the range between the anchor
and the release in *drawn* order, and Up/Down step one row while the list owns
the keyboard (a click in the drawing hands it back). Double-click toggles
Always-Show on that layer; Ctrl/Shift+double-click toggles it on the whole
selection. Held layers stay visible whatever is selected, and nothing else is
protected: the current layer is a row like any other, so the *Layer 0 always on*
pin is the only guard. A row dims when the walk hides it and is only muted when
the drawing itself has it off or frozen. The header's second line reports how
many of the listed layers are on screen.

Counts and purgeability come from the same pass, using PURGE's rules verbatim,
so a row's number and its purgeability agree with `PURGE LAyers`. System layers
(`*…`) are not listed, and a `*`-prefixed layer is unioned into the set so the
walk can never hide it by omission.

## Filtering

Typing in the filter box does nothing until Enter. Enter applies the pattern
through the app's existing wildcard matcher and highlights the matches; the
Filter checkbox then narrows the list to them, and is disabled while no filter
is active. The dropdown lists saved filters only — **Save Current Filter** adds
the active pattern, **Delete Current Filter** removes it again, and **Copy as
Filter** seeds the box from the first selected layer.

## Actions

The context menu is the spec's list, in order: Hold Selection, Release
Selection, Release All, Select All, Clear All, Invert Selection, Select
Unreferenced, Save Layer State…, Inspect…, Copy as Filter, Save Current Filter,
Delete Current Filter, Zoom to Layer Extents, Select Objects on Layer. Disabled
rows stay visible with their reason, so the menu keeps its shape.

- **Select Objects** (header) hides the pallet, runs the standard
  object-selection prompt and restores it; the picked objects' layers replace
  the selection.
- **Purge** is enabled only when the selection contains purgeable layers, asks
  for confirmation, and refreshes rows and counts afterwards.
- **Save Layer State…** opens the existing editor pre-filled with the walk's
  visibility, so a state saved mid-walk records what the walk is showing rather
  than what the table says.
- **Inspect…** reports total layers, selected layers and objects on the
  selected layers.

## Localisation

All user-visible strings go through Fluent under the `layer-walk` message id in
every locale, with English as the source. Counts use `$count` plural selectors,
and `LayerWalk` / `Layer Walk` stay untranslated as the command's name — only
the `Layers: N` suffix of the pallet title is localised.
