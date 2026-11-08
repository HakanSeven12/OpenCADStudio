---
summary: Opens the Drawing Units dialog, where you set how lengths and angles are written and what one drawing unit measures.
related: DDUNITS, DWGUNITS, UN
---
## Usage
1. Run `UNITS`. The Drawing Units dialog opens.
2. Change the settings. The **Sample output** shows a length and an angle in the chosen formats as you go.
3. Click **OK** to apply, or **Cancel** to leave the drawing unchanged.

## Dialog
| Field | What it does |
|---|---|
| **Length › Type** | Architectural, Decimal, Engineering, Fractional or Scientific. Sets [`LUNITS`](LUNITS-System-Variable). |
| **Length › Precision** | 0 to 8 decimal places. For Architectural and Fractional it is the smallest fraction (1/2 to 1/256). Sets [`LUPREC`](LUPREC-System-Variable). |
| **Angle › Type** | Decimal Degrees, Deg/Min/Sec, Grads, Radians or Surveyor's Units. Sets [`AUNITS`](AUNITS-System-Variable). |
| **Angle › Precision** | 0 to 8 places. Sets [`AUPREC`](AUPREC-System-Variable). |
| **Zero at … degrees** | The direction of angle zero. Sets [`ANGBASE`](ANGBASE-System-Variable). |
| **Positive angles run clockwise** | Measures angles clockwise instead of counterclockwise. Sets [`ANGDIR`](ANGDIR-System-Variable). |
| **Insertion scale › Unit** | What one drawing unit measures (for example Millimeters). Blocks and drawings inserted from elsewhere are scaled by it; Unitless inserts them unscaled. Sets [`INSUNITS`](INSUNITS-System-Variable). |

## Notes
- The dialog only changes how numbers are written and how inserted content is scaled. It never moves or resizes geometry; use [`DWGUNITS`](DWGUNITS-Command) for that.
- The settings are stored in the drawing.
- The units button on the status bar changes the length format only.
