---
summary: Asks which geometric constraint to apply, then runs that constraint command.
related: GCHORIZONTAL, GCVERTICAL, GCPARALLEL, GCPERPENDICULAR, GCTANGENT, GCCOINCIDENT, GCFIX
---

## Usage
1. Enter constraint type: type one of the options, or press Enter to repeat the type shown in angle brackets.
2. Follow the prompts of the chosen constraint.

## Options
| Option | What it does |
|---|---|
| **Horizontal** (`H`) | Runs [`GCHORIZONTAL`](GCHORIZONTAL-Command). |
| **Vertical** (`V`) | Runs [`GCVERTICAL`](GCVERTICAL-Command). |
| **Perpendicular** (`P`) | Runs [`GCPERPENDICULAR`](GCPERPENDICULAR-Command). |
| **Parallel** (`PA`) | Runs [`GCPARALLEL`](GCPARALLEL-Command). |
| **Tangent** (`T`) | Runs [`GCTANGENT`](GCTANGENT-Command). |
| **Smooth** (`SM`) | Runs [`GCSMOOTH`](GCSMOOTH-Command). |
| **Coincident** (`C`) | Runs [`GCCOINCIDENT`](GCCOINCIDENT-Command). |
| **Concentric** (`CON`) | Runs [`GCCONCENTRIC`](GCCONCENTRIC-Command). |
| **Collinear** (`COL`) | Runs [`GCCOLLINEAR`](GCCOLLINEAR-Command). |
| **Symmetric** (`SY`) | Runs [`GCSYMMETRIC`](GCSYMMETRIC-Command). |
| **Equal** (`E`) | Runs [`GCEQUAL`](GCEQUAL-Command). |
| **Fix** (`F`) | Runs [`GCFIX`](GCFIX-Command). |

## Notes
- The default type is the last geometric constraint you used, whether you chose it here or ran it directly. At first it is Coincident.
- Objects you selected before starting stay selected, so the chosen constraint can use them.
