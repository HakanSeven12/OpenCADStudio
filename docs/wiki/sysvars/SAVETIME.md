---
summary: Sets the automatic save interval in minutes; 0 turns automatic save off.
type: Integer
saved: Program settings
default: 10 (minutes)
related: SAVEFILEPATH, SAVE, QSAVE
---

## Values
Any whole number of minutes from 0 up. 0 turns automatic save off. The slider in the Options dialog goes from 0 to 120.

## Notes
- At each interval, every drawing with unsaved changes is written as a `.sv$` recovery copy to the folder set by [`SAVEFILEPATH`](SAVEFILEPATH-System-Variable). Your drawing file itself is not changed.
- In the web version the copies are kept in the browser's storage instead.
- Set it in Options › Open and Save › Automatic save, or type `SAVETIME` and a value.
