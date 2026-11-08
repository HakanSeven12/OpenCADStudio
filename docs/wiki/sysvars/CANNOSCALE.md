---
summary: Sets the current annotation scale, which sizes annotative objects.
type: String
saved: Drawing
default: 1:1
related: ANNOSCALE, ANNOALLVISIBLE, ANNOAUTOSCALE, MSLTSCALE
---

## Values
A scale from the drawing's scale list, such as 1:50 or 2:1, or a plain factor.

## Notes
- [`ANNOSCALE`](ANNOSCALE-Command) sets the same value. Typing `CANNOSCALE` alone starts the same prompt.
- When [`ANNOAUTOSCALE`](ANNOAUTOSCALE-System-Variable) is on, a new scale is also added to existing annotative objects.
