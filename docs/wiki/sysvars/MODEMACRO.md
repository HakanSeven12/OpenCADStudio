---
summary: Shows your own text, worked out by a DIESEL string, at the left end of the status bar.
type: String
saved: Not saved
default: "" (nothing shown)
related: SETVAR, FIELD
---

## Values
Any text. Parts written as DIESEL functions, `$(function,argument,…)`, are replaced by their value each time the status bar is drawn. For example `Layer: $(getvar,clayer)` shows the current layer, and `$(edtime,$(getvar,date),HH:MM)` the time.

## Notes
- Typed alone, it asks for the new string. `SETVAR MODEMACRO string` sets it directly. Type `.` to clear it.
- A part that cannot be worked out (an unknown variable, a bad argument) shows `$?`.
- The same DIESEL functions work in a command of your own ribbon panels ([`CUI`](CUI-Command)) that starts with `$M=`: the macro that runs is the string's value, for example `$M=$(if,$(eq,$(getvar,clayer),0),-LAYER;SET;Notes;;,LINE)`.
- It lasts for the current session only and starts empty each time the program starts.
