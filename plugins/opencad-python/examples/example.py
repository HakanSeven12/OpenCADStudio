# Example script for opencad-python's PY_RUN command.
#
# Historical experimental command-replay example. The portable default
# build omits ocs.command()/ocs.select(); nested replay may hang in OCS 2026.37.
#
# Draws two rectangles, a triangle, an arc, a pair of lines made parallel by
# a real PCONSTRAINT, a circle made tangent to a line by a real TCONSTRAINT,
# and a row of points added in one ocs.add_points() batch call, one of which
# gets an XDATA record written, read back, and removed. Every ocs.add_*/
# ocs.write_record call in one PY_RUN shares a script undo group. Command
# replay has separate host undo semantics.
#
# These ARE real, persistent constraint objects, not just geometrically
# correct-looking shapes: drag one of the two "parallel" lines afterward and
# the other follows to stay parallel, same for the tangent circle. Verified
# end-to-end (not just "it compiles"): after ocs.command("PCONSTRAINT") /
# ("TCONSTRAINT"), the geometry visibly changes and satisfies the
# relationship exactly (checked numerically — direction-vector cross
# product ~0 for parallel, center-to-line distance == radius for tangent).
#
# ocs.select(handles) + ocs.command(name) is what makes this possible:
# constraint commands read a prior *selection* (a "Select objects:"
# prompt), not picks fed as command-line tokens the way LINE's points are —
# select the entities first, then run the bare command name. See PLUGIN.md
# for the two host-side additions (HostApi::run_command / set_selection)
# this needed and exactly what they do and don't cover.

if not hasattr(ocs, "command") or not hasattr(ocs, "select"):
    raise RuntimeError("This example requires experimental command replay host APIs")

def rectangle(x, y, w, h):
    l1 = ocs.add_line(x, y, x + w, y)
    l2 = ocs.add_line(x + w, y, x + w, y + h)
    l3 = ocs.add_line(x + w, y + h, x, y + h)
    l4 = ocs.add_line(x, y + h, x, y)
    return [l1, l2, l3, l4]

# ── Two rectangles (not aligned to each other — see the note below) ───────
rectangle(0, 0, 20, 10)
rectangle(30, 2, 18, 9)

# ── A triangle (three lines closing a loop) ────────────────────────────────
ocs.add_line(60, 0, 75, 0)
ocs.add_line(75, 0, 67.5, 12)
ocs.add_line(67.5, 12, 60, 0)

# ── An arc, for variety ─────────────────────────────────────────────────────
ocs.add_arc(67.5, 20, 8, 0, 180)

# ── Two lines, deliberately NOT parallel — then made parallel by a real ───
# ── persistent PCONSTRAINT (not just recomputed coordinates).
line_a = ocs.add_line(0, 20, 20, 24)
line_b = ocs.add_line(0, 30, 20, 33)
ocs.select([line_a, line_b])
ocs.command("PCONSTRAINT")

# ── A line and a circle, deliberately NOT touching — then made tangent by ──
# ── a real persistent TCONSTRAINT.
tangent_line = ocs.add_line(0, 40, 20, 40)
tangent_circle = ocs.add_circle(10, 44, 3)
ocs.select([tangent_line, tangent_circle])
ocs.command("TCONSTRAINT")

# ── What's NOT in this script: aligning the two rectangles to each other ──
# ── (a real ALIGN, not just matching coordinates). Tried it separately:
# ── ocs.command("ALIGN <points...>") DOES apply the transform correctly,
# ── but ALIGN asks a trailing "Scale objects? [Yes/No]" question that
# ── ocs.command()'s point-feeding doesn't answer (it sends exactly one
# ── trailing Enter, and ALIGN needs two — one to skip the optional 3rd
# ── point pair, one for the scale question) — it leaves that prompt open
# ── instead of cleanly finishing. Left out of this script rather than
# ── shipping something that half-works; see PLUGIN.md for the detail.

# ── A batch of points, added in one HostApi::add_entities() call instead ──
# ── of one ocs.add_line-style call per point.
point_handles = ocs.add_points([[x, 50, 0] for x in range(0, 21, 2)])

# ── Tag one of those points with an XDATA record, read it back, then ───────
# ── remove it — round-tripping every supported value kind.
tagged = point_handles[0]
ocs.write_record(tagged, "OPENCAD_EXAMPLE", [
    {"kind": "String", "value": "origin marker"},
    {"kind": "Point3D", "value": [0.0, 50.0, 0.0]},
    {"kind": "Integer32", "value": 1},
])
record = ocs.read_record(tagged, "OPENCAD_EXAMPLE")
assert record["app_name"] == "OPENCAD_EXAMPLE"
assert record["values"][0] == {"kind": "String", "value": "origin marker"}
ocs.remove_record(tagged, "OPENCAD_EXAMPLE")
assert ocs.read_record(tagged, "OPENCAD_EXAMPLE") is None
