# Dialogs, tool windows and windows of their own

Open CAD Studio draws every dialog inside the main window, so the desktop and
the web build share one code path. Two kinds of dialog live there.

- A **modal dialog** is one at a time, over a dimmed backdrop, and the drawing
  waits until it is answered.
- A **tool dialog** opens as a frame over the drawing without a backdrop. The
  drawing, the command line, the dock pallets and other tool dialogs keep
  working while it is open. Several tool dialogs can be open at once; a press
  on one brings it to the front. On the desktop a tool dialog, and any dock
  pallet, can move into a **window of its own**, for example onto a second
  monitor. The web build has one canvas, so there they stay frames inside it.

## Which dialogs are tools

`ModalKind::tool_scope` decides. A tool's scope says what a drawing switch does
to it.

| Scope | Dialogs | On a drawing switch |
|---|---|---|
| Program | About, Shortcuts, Aliases, Plugin Manager, Options, Reference Manager help | stay open |
| Drawing | Drafting Settings, Layer States Manager, Layer Translator, Drawing Units, the Text / Table / Multiline / Multileader / Dimension / Point style managers, Scale List, Layout Manager, Drawing Properties, Rename, Purge, Find and Replace, Spelling, Data Link Manager, Block Attribute Manager, Select Similar settings, Block Editor settings, AutoConstrain settings | close |

Everything else stays modal:

- confirmations and warnings;
- file, recovery and save flows;
- dialogs tied to a running command or an object pick (Plot, Insert, Block
  Definition, table cells, attributes, …);
- child dialogs other dialogs return to (Plot Style from Plot, Style Import
  from a style manager, Options opened from a PDF dialog, Table Style from
  Insert Table).

A tool dialog opened as such a child stays modal for that opening.

## How it works

Dialog code keys everything on `active_modal`: the dialog its messages work on
and the one its close path closes. Tools keep that code unchanged.

- Every message from a tool's frame or window arrives wrapped as
  `Message::ToolWindow(kind, message)`. The tool is swapped in as the active
  modal (`enter_tool`) and the message runs as it always did.
- After every update (`update_root` → `settle_tool_windows`):
  - a tool swapped in that is no longer the active modal was closed, or
    replaced by the dialog it opened, and is dropped;
  - a tool left as the active modal is parked again, keeping where its frame
    is;
  - on the desktop, windows of their own open and close to match what is
    open.
- Automation drives a parked tool the same way: `focus_dialog` swaps it in for
  the request.
- A modal dialog covers the tool frames, and while it is up their messages are
  ignored.

## Windows of their own (desktop)

- A tool frame's title bar has an "Open in its own window" button. A floating
  pallet's title-bar menu has the same entry.
- The window's header has "Back to the main window", which returns a tool to
  its frame and floats a pallet in the main window again.
- Closing the window from the window manager closes the tool or hides the
  pallet.
- The next open of either comes back in its own window, where it last was.
  Placements are kept in the dock section of the configuration
  (`detached` for pallets, `tool_windows` for dialogs).
- Keys typed in a window of its own stay with its widgets. They do not reach
  the command line or the drawing's shortcuts.

## Command History pallet

The **Command History** pallet (`COMMANDHISTORY`, or the dock's pallet menu)
keeps the whole command log open beside the drawing:

- one read-only editor, so you can select across lines and copy with Ctrl+C;
- Copy and Clear buttons;
- it can dock, float, or go into a window of its own like any other pallet.
