# Dialogs, tool windows and windows of their own

Every dialog is a **tool dialog**: it never blocks the drawing. The desktop
and the web build share one code path for it.

- The drawing, the command line, the dock pallets and other dialogs keep
  working while it is open.
- Several dialogs can be open at once. A press on one brings it to the front.
- On the desktop every dialog opens in a **window of its own**, sized to its
  content, so it can sit anywhere, for example on a second monitor.
- Dock pallets stay in the dock. Any pallet can move into a window of its own
  from its title bar, and back.
- A manager that is both a dialog and a pallet opens by where it is called
  from: its command (typed, or from the ribbon, a menu, the Quick Access
  Toolbar) opens it in a window of its own on the desktop; the dock's pallet
  menu opens it as a pallet on that edge. One already showing stays where it
  is. These are Layer Manager (LAYERS), External References, QuickCalc,
  Sheet Set Manager, Count (COUNTLIST), Layer Walk, Point Cloud Manager and
  Command History.
- The web build has one canvas, so there dialogs and pallets stay as frames
  inside it.

## Scope: what a drawing switch does

`ModalKind::tool_scope` decides.

| Scope | Dialogs (`ModalKind`) | On a drawing switch |
|---|---|---|
| Program | About, Shortcuts, Aliases, PluginManager, Options, XrefHelp, UpdateNotice, DonationPrompt, GpuWarning, AssocPrompt, Unsaved, BlockEditChanges, SaveDialog, Recovery, RecoveryPrompt, AutosaveRestore, ConversionLoss, AecDropWarning, FileInUse, ExternalChange, ImportFile, DgnImport, TdsImport, TdsMaterial, TdsMissingMap, PdfImportFile, PdfPassword, SheetSet, PluginDialog, ExtractionOutdated, DataExtractionUnsaved | stays open: program settings and notices, and flows that hold a file or tab operation half done |
| Drawing | every other dialog | closes: it works on the current drawing or on objects picked in it |

## The keyboard

A dialog that opens has the keyboard, as a modal dialog had:

- Esc closes it.
- Enter answers it where Enter did before (Block Definition, Write Block, the
  attribute form, the block questions).
- Other keys stay out of the command line.

A press on a dialog's frame gives it the keyboard. A press on the drawing, on
a viewport pane, on the ribbon or into the command line gives the keyboard
back to the drawing.

## Dialogs opened from dialogs

A dialog opened from another one (Plot Style from Plot, Style Import from a
style manager, Options from a PDF dialog, Table Style from Insert Table)
replaces it while it is up. Closing it brings the first dialog back.

The colour picker is the exception. It is drawn over the dialog it was opened
from and answers into it, so while it is up that dialog stays modal.

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
- What a tool's message starts (a file read, a save, a pick) answers as
  `Message::ToolWindowTask(kind, message)`. It runs with the tool swapped in
  again while the tool is still open, as it did while that dialog was modal.
- Automation drives a parked tool the same way: `focus_dialog` swaps it in for
  the request. The generic buttons (`dialog_ok`, `dialog_cancel`, …) press
  the front-most dialog.

## Windows of their own (desktop)

- A dialog is laid out once off screen, then its window opens at that size.
  A dialog whose window was moved or resized opens there, at that size.
- Esc in a dialog's window closes it. Enter answers it where Enter did before
  in the main window, unless a field took the Enter.
- The colour picker a dialog opens is drawn over that dialog in its window.
- A docked pallet's title bar, and a floating pallet's title-bar menu, have
  "Open in its own window". The pallet's window has "Back to the main
  window", which floats it in the main window again. A dialog's window is
  only the dialog.
- Closing the window from the window manager closes the dialog or hides the
  pallet.
- The next open of either comes back where it last was. Placements are kept
  in the dock section of the configuration: `detached` for pallets,
  `tool_windows` for dialogs.
- Other keys typed in a window of its own stay with its widgets. They do not
  reach the command line or the drawing's shortcuts.

## Command History pallet

The **Command History** pallet (`COMMANDHISTORY`, or the dock's pallet menu)
keeps the whole command log open beside the drawing:

- one read-only editor, so you can select across lines and copy with Ctrl+C;
- Copy and Clear buttons;
- it can dock, float, or go into a window of its own like any other pallet.
