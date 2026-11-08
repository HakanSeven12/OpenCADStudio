---
summary: Creates a multileader: an arrowhead, a leader line and a landing that point to text or a block.
related: MLEADERSTYLE, MLEADEREDIT, MLEADERALIGN, MLEADERCOLLECT, LEADER, QLEADER
ribbon: Annotate tab › Leaders panel
---

## Usage
1. Specify the leader arrowhead location, or type an option.
2. Specify the next point. Leader points are added until the maximum number of points is reached, or you press Enter.
3. With text content, type the text in the text editor that opens at the landing.

The command remembers which creation order you used last (arrowhead first, landing first or content first) and starts with it next time.

## Options
At the first prompt:

| Option | What it does |
|---|---|
| **pre enter Text** (`T`) | Type the text first, then place the leader. |
| **leader Landing first** (`L`) | Pick the landing location first, then the arrowhead. |
| **leader arrowHead first** (`H`) | Pick the arrowhead first, then the landing. |
| **Content first** (`C`) | Pick two corners of the text box first, then the arrowhead. |
| **select Mtext** (`M`) | In content-first order: use an existing multiline text object as the content. |
| **Options** (`O`) | Opens the options below. Pressing Enter at the first prompt does the same. |

Options:

| Option | What it does |
|---|---|
| **Leader type** (`L`) | Straight, sPline (curved) or None (no line shown). |
| **leader lAnding** (`A`) | Turn the landing on or off, and set its fixed length. |
| **Content type** (`C`) | Block, Mtext or None. With Block, enter the block name. |
| **Maxpoints** (`M`) | The most points a leader line can have. |
| **First angle** (`F`) | Angle constraint for the first segment: 0, 15, 30, 45, 60, 90 or 180. |
| **Second angle** (`S`) | Angle constraint for the second segment, same values. |
| **laYer** (`Y`) | The layer new multileaders go on: type a name, select an object to use its layer, or type `.` for the current layer. `?` lists layers. |
| **eXit options** (`X`) | Returns to placing the leader. |

## Notes
- Default settings come from the current multileader style. Choose it in the style list on the Leaders panel, or edit it with [`MLEADERSTYLE`](MLEADERSTYLE-Command).
- Press Enter after two or more points to finish the leader early.
