"""Thin Python view over OCS host API v7 entity transactions and coverage."""

def _coordinates(value, width):
    return dict(zip("xyz"[:width], (float(axis) for axis in value)))


def _coerce_points(kind, properties):
    """Let create_entity take (x, y, z) tuples for point properties, as
    transaction edits already do, by using the property types in the schema."""
    schema = ocs.entity_coverage(kind)
    types = {row["name"]: row for row in schema.get("properties", [])} if schema else {}
    result = {}
    for name, value in properties.items():
        row = types.get(name)
        if row is not None and row["type"] in ("Vector2", "Vector3"):
            width = 2 if row["type"] == "Vector2" else 3
            if row["sequence"]:
                value = [_coordinates(item, width) if isinstance(item, (tuple, list)) else item for item in value]
            elif isinstance(value, (tuple, list)):
                value = _coordinates(value, width)
        result[name] = value
    return result


class _Entity:
    def __init__(self, document, handle):
        object.__setattr__(self, "_document", document)
        object.__setattr__(self, "handle", handle)

    def _data(self):
        data = ocs.entity_descriptor(self.handle)
        if data is None:
            raise KeyError("entity %s is absent or outside the Python schema" % self.handle)
        return data

    def __getattr__(self, name):
        data = self._data()
        if name not in data:
            raise AttributeError("property %s is not in the Python entity schema" % name)
        pending = self._document._pending or {}
        value = pending.get(self.handle, {}).get(name, data[name])
        if isinstance(value, dict) and all(axis in value for axis in ("x", "y")):
            return tuple(value[axis] for axis in ("x", "y", "z") if axis in value)
        return value

    @property
    def coverage(self):
        return ocs.entity_coverage(self._data()["kind"])

    @property
    def snapshot(self):
        """Detached copy of all serialized CAD fields for this entity."""
        data = ocs.entity_snapshot(self.handle)
        if data is None:
            raise KeyError("entity %s is absent" % self.handle)
        return data[self._data()["kind"]]

    def __setattr__(self, name, value):
        if self._document._pending is None:
            raise RuntimeError("entity writes require doc.transaction(...)")
        data = self._data()
        can_edit = name in self.coverage["editable"]
        if not can_edit or name not in data or name in ("kind", "handle", "_editable"):
            raise AttributeError("property %s is not editable through this schema" % name)
        old = data[name]
        if isinstance(old, dict) and all(axis in old for axis in ("x", "y")):
            axes = tuple(axis for axis in ("x", "y", "z") if axis in old)
            if not isinstance(value, (tuple, list)) or len(value) != len(axes):
                raise ValueError("%s expects %s coordinates" % (name, len(axes)))
            value = dict(zip(axes, value))
        self._document._pending.setdefault(self.handle, {})[name] = value


class _Entities:
    def __init__(self, document):
        self._document = document

    def __getitem__(self, handle):
        entity = _Entity(self._document, int(handle))
        entity._data()
        return entity

    def __iter__(self):
        for handle in ocs.entity_handles():
            yield self[handle]

    def __len__(self):
        return len(ocs.entity_handles())


class _Transaction:
    def __init__(self, document, label):
        self.document, self.label = document, label

    def __enter__(self):
        if self.document._pending is not None:
            raise RuntimeError("nested document transactions are unsupported")
        self.document._pending = {}
        return self.document

    def __exit__(self, exc_type, exc, tb):
        updates = self.document._pending
        self.document._pending = None
        if exc_type is None and updates:
            ocs.update_many(self.label, [dict(handle=handle, **patch) for handle, patch in updates.items()])
        return False


class _Solids:
    """Kernel-backed solid creation and rigid moves.

    The host builds and edits the geometry; a script never sees the ACIS
    payload. Every call is its own operation in the script's undo group and
    fails, leaving the drawing unchanged, when the kernel cannot do it.
    """

    def __init__(self, document):
        self._document = document

    def _create(self, primitive, values, layer):
        handle = ocs.solid_create(primitive, [float(v) for v in values], layer)
        return self._document.entities[handle]

    def box(self, center=(0, 0, 0), size=(1, 1, 1), layer=None):
        return self._create("box", tuple(center) + tuple(size), layer)

    def wedge(self, origin=(0, 0, 0), size=(1, 1, 1), layer=None):
        return self._create("wedge", tuple(origin) + tuple(size), layer)

    def cylinder(self, center=(0, 0, 0), radius=1, height=1, layer=None):
        return self._create("cylinder", tuple(center) + (radius, height), layer)

    def sphere(self, center=(0, 0, 0), radius=1, layer=None):
        return self._create("sphere", tuple(center) + (radius,), layer)

    def torus(self, center=(0, 0, 0), major=2, minor=0.5, layer=None):
        return self._create("torus", tuple(center) + (major, minor), layer)

    def pyramid(self, center=(0, 0, 0), radius=1, height=1, sides=4, layer=None):
        return self._create("pyramid", tuple(center) + (radius, height, sides), layer)

    def region(self, profile, layer=None, delete_source=False):
        """Make a planar region from one closed planar profile (a circle,
        ellipse, closed polyline or spline). Keeps the profile unless
        `delete_source` is true, and uses the profile's layer by default."""
        handle = profile.handle if isinstance(profile, _Entity) else int(profile)
        return self._document.entities[ocs.solid_region(handle, layer, bool(delete_source))]

    def _boolean(self, operation, first, second, layer, keep_operands):
        a = first.handle if isinstance(first, _Entity) else int(first)
        b = second.handle if isinstance(second, _Entity) else int(second)
        return self._document.entities[ocs.solid_boolean(a, b, operation, layer, bool(keep_operands))]

    def union(self, first, second, layer=None, keep_operands=False):
        """Join two solids into a new one. The operands are consumed unless
        `keep_operands`. The kernel may refuse (the error says why) and a
        refusal changes nothing. Curved operands can take a second or two,
        and a whole script has a 30 second budget."""
        return self._boolean("union", first, second, layer, keep_operands)

    def subtract(self, first, second, layer=None, keep_operands=False):
        """Remove `second` from `first`, giving a new solid."""
        return self._boolean("subtract", first, second, layer, keep_operands)

    def intersect(self, first, second, layer=None, keep_operands=False):
        """Keep only the volume both solids share, as a new solid."""
        return self._boolean("intersect", first, second, layer, keep_operands)

    def surface(self, profile, layer=None, delete_source=False):
        """Make a plane surface from one closed planar profile."""
        handle = profile.handle if isinstance(profile, _Entity) else int(profile)
        return self._document.entities[ocs.solid_surface(handle, layer, bool(delete_source))]

    def extrude(self, profile, direction, layer=None, delete_source=False):
        """Extrude a planar profile along `direction`: a Solid3D when the
        profile is closed, a Surface when it is open."""
        handle = profile.handle if isinstance(profile, _Entity) else int(profile)
        vector = [float(v) for v in direction]
        return self._document.entities[ocs.solid_extrude(handle, vector, layer, bool(delete_source))]

    def transform(self, entity, matrix):
        """Apply a column-major 4x4 rigid transform (16 numbers) in place."""
        handle = entity.handle if isinstance(entity, _Entity) else int(entity)
        ocs.solid_transform(handle, [float(v) for v in matrix])
        return self._document.entities[handle]

    def translate(self, entity, offset):
        dx, dy, dz = (float(v) for v in offset)
        return self.transform(entity, [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, dx, dy, dz, 1])

    def rotate(self, entity, axis, angle, about=(0, 0, 0)):
        """Turn about the world axis 'x', 'y' or 'z' through `about`."""
        index = {"x": 0, "y": 1, "z": 2}[axis]
        return self.transform(entity, ocs.rotation_matrix(index, float(angle), [float(v) for v in about]))

    def mirror(self, entity, axis, about=(0, 0, 0)):
        """Reflect across the plane normal to the world axis through `about`."""
        index = {"x": 0, "y": 1, "z": 2}[axis]
        return self.transform(entity, ocs.mirror_matrix(index, [float(v) for v in about]))


class _Layers:
    """The drawing's layer table. Every change goes through the host: it is
    validated, becomes one undo step, and a refusal raises `RuntimeError`
    without changing the drawing."""

    _KEYS = ("color", "linetype", "lineweight", "off", "frozen", "locked",
             "plottable", "transparency", "description")

    def _records(self):
        return ocs.layer_records()

    def __iter__(self):
        return iter(self._records())

    def __len__(self):
        return len(self._records())

    def __contains__(self, name):
        wanted = str(name).strip().upper()
        return any(r["name"].upper() == wanted for r in self._records())

    def __getitem__(self, name):
        wanted = str(name).strip().upper()
        for record in self._records():
            if record["name"].upper() == wanted:
                return record
        raise KeyError(name)

    def get(self, name, default=None):
        try:
            return self[name]
        except KeyError:
            return default

    def names(self):
        return [r["name"] for r in self._records()]

    @property
    def current(self):
        for record in self._records():
            if record["current"]:
                return record
        return None

    @current.setter
    def current(self, name):
        self.set_current(name)

    def _options(self, properties):
        unknown = [k for k in properties if k not in self._KEYS]
        if unknown:
            raise TypeError("unknown layer propert%s: %s" % ("y" if len(unknown) == 1 else "ies", ", ".join(sorted(unknown))))
        return {k: v for k, v in properties.items() if v is not None}

    def create(self, name, **properties):
        """Create a layer and return its record. `color` is an ACI index
        (1-255), an (r, g, b) tuple or a Color dict; `lineweight` is in
        1/100 mm (-1 ByLayer, -2 ByBlock, -3 Default); `transparency` is a
        percentage 0-90."""
        ocs.layer_operation("create", str(name), self._options(properties))
        return self[name]

    def modify(self, name, **properties):
        """Change only the properties given and return the updated record."""
        ocs.layer_operation("modify", str(name), self._options(properties))
        return self[name]

    def rename(self, name, new_name):
        ocs.layer_operation("rename", str(name), {"to": str(new_name)})
        return self[new_name]

    def delete(self, name, erase_objects=False):
        """Delete a layer. A layer that still holds objects is refused unless
        `erase_objects=True`. Layer 0, Defpoints and the current layer stay."""
        ocs.layer_operation("delete", str(name), {"erase_objects": bool(erase_objects)})

    def set_current(self, name):
        ocs.layer_operation("set_current", str(name), None)
        return self[name]


class _Styles:
    """Common lookup, rename, delete and current-style handling for the text
    and dimension style tables. Every change is validated by the host, is one
    undo step (making a style current is a setting), and a refusal raises
    `RuntimeError` without changing the drawing. `Standard` is never renamed
    or deleted, and a style that is current or in use is never deleted."""

    _KIND = None
    _RECORDS = None

    def _records(self):
        return getattr(ocs, self._RECORDS)()

    def __iter__(self):
        return iter(self._records())

    def __len__(self):
        return len(self._records())

    def __contains__(self, name):
        wanted = str(name).strip().upper()
        return any(r["name"].upper() == wanted for r in self._records())

    def __getitem__(self, name):
        wanted = str(name).strip().upper()
        for record in self._records():
            if record["name"].upper() == wanted:
                return record
        raise KeyError(name)

    def get(self, name, default=None):
        try:
            return self[name]
        except KeyError:
            return default

    def names(self):
        return [r["name"] for r in self._records()]

    @property
    def current(self):
        for record in self._records():
            if record["current"]:
                return record
        return None

    @current.setter
    def current(self, name):
        self.set_current(name)

    def _run(self, op, name, options=None):
        ocs.style_operation(self._KIND, op, str(name), options)

    def rename(self, name, new_name):
        self._run("rename", name, {"to": str(new_name)})
        return self[new_name]

    def delete(self, name):
        self._run("delete", name)

    def set_current(self, name):
        self._run("set_current", name)
        return self[name]


class _TextStyles(_Styles):
    """`ocs.active_document.text_styles`. Properties: `height` (0 = each text
    chooses), `width_factor`, `oblique` (degrees, within 85), `font` (SHX file),
    `big_font`, `backward`, `upside_down`, `vertical`, `annotative`. Give a
    TrueType face as a font file name such as `font="arial.ttf"`: the codec does
    not persist a separate TrueType family name, so it is not exposed."""

    _KIND = "text"
    _RECORDS = "text_style_records"
    _KEYS = ("height", "width_factor", "oblique", "font", "big_font",
             "backward", "upside_down", "vertical", "annotative")

    def _options(self, properties):
        unknown = [k for k in properties if k not in self._KEYS]
        if unknown:
            raise TypeError("unknown text style propert%s: %s" % ("y" if len(unknown) == 1 else "ies", ", ".join(sorted(unknown))))
        return {k: v for k, v in properties.items() if v is not None}

    def create(self, name, **properties):
        self._run("create", name, self._options(properties))
        return self[name]

    def modify(self, name, **properties):
        self._run("modify", name, self._options(properties))
        return self[name]


class _DimStyles(_Styles):
    """`ocs.active_document.dim_styles`. Records and properties use the DimStyle
    field names (`dimscale`, `dimtxt`, `dimasz`, `dimtxsty` ...); handles and
    xref fields are host-managed and refused. `create(..., copy_from="Other")`
    starts from an existing style."""

    _KIND = "dim"
    _RECORDS = "dim_style_records"

    def _options(self, properties):
        return {k: v for k, v in properties.items() if v is not None or k == "dimclrd_true_color"}

    def create(self, name, copy_from=None, **properties):
        options = self._options(properties)
        if copy_from is not None:
            options["copy_from"] = str(copy_from)
        self._run("create", name, options)
        return self[name]

    def modify(self, name, **properties):
        self._run("modify", name, self._options(properties))
        return self[name]


class _Blocks:
    """`ocs.active_document.blocks`: user block definitions (layout and
    anonymous blocks are not listed). Every change is validated by the host, is
    one undo step, and a refusal raises `RuntimeError` without changing the
    drawing. Place a block with `create_entity("Insert", block_name=...)`."""

    def _records(self):
        return ocs.block_records()

    def __iter__(self):
        return iter(self._records())

    def __len__(self):
        return len(self._records())

    def __contains__(self, name):
        wanted = str(name).strip().upper()
        return any(r["name"].upper() == wanted for r in self._records())

    def __getitem__(self, name):
        wanted = str(name).strip().upper()
        for record in self._records():
            if record["name"].upper() == wanted:
                return record
        raise KeyError(name)

    def get(self, name, default=None):
        try:
            return self[name]
        except KeyError:
            return default

    def names(self):
        return [r["name"] for r in self._records()]

    def create(self, name, entities, base_point=(0, 0, 0), erase_originals=False, description=None):
        """Define a block from existing model/paper-space entities (descriptors
        or handles). They are copied into the definition shifted by
        `-base_point`; `erase_originals=True` removes them from the drawing, as
        the BLOCK command does. No insert is placed."""
        handles = [e.handle if isinstance(e, _Entity) else int(e) for e in entities]
        options = {"entities": handles, "base_point": [float(v) for v in base_point],
                   "erase_originals": bool(erase_originals)}
        if description is not None:
            options["description"] = str(description)
        ocs.block_operation("create", str(name), options)
        return self[name]

    def modify(self, name, description=None, explodable=None, scale_uniformly=None):
        options = {k: v for k, v in (("description", description), ("explodable", explodable),
                                     ("scale_uniformly", scale_uniformly)) if v is not None}
        ocs.block_operation("modify", str(name), options)
        return self[name]

    def rename(self, name, new_name):
        """Rename a block; every insert of it follows."""
        ocs.block_operation("rename", str(name), {"to": str(new_name)})
        return self[new_name]

    def delete(self, name):
        """Delete a block that no insert, style or leader still uses."""
        ocs.block_operation("delete", str(name), None)


class _Named:
    """Shared lookup for the linetype and layout collections."""

    _RECORDS = None

    def _records(self):
        return getattr(ocs, self._RECORDS)()

    def __iter__(self):
        return iter(self._records())

    def __len__(self):
        return len(self._records())

    def __contains__(self, name):
        wanted = str(name).strip().upper()
        return any(r["name"].upper() == wanted for r in self._records())

    def __getitem__(self, name):
        wanted = str(name).strip().upper()
        for record in self._records():
            if record["name"].upper() == wanted:
                return record
        raise KeyError(name)

    def get(self, name, default=None):
        try:
            return self[name]
        except KeyError:
            return default

    def names(self):
        return [r["name"] for r in self._records()]


class _Linetypes(_Named):
    """`ocs.active_document.linetypes`. A pattern is signed lengths in drawing
    units: positive is a dash, negative a gap, zero a dot (2-12 elements with at
    least one gap and one dash or dot). Text and shape linetypes can be read
    but not created or edited. Every change is validated by the host, is one
    undo step, and a refusal raises `RuntimeError` without changing anything.
    `Continuous`, `ByLayer` and `ByBlock` are never changed; a linetype used by
    a layer, an entity, a dimension style or the current setting is never
    deleted."""

    _RECORDS = "linetype_records"

    def create(self, name, pattern, description=""):
        ocs.linetype_operation("create", str(name), {"pattern": [float(v) for v in pattern],
                                                     "description": str(description)})
        return self[name]

    def modify(self, name, pattern=None, description=None):
        options = {}
        if pattern is not None:
            options["pattern"] = [float(v) for v in pattern]
        if description is not None:
            options["description"] = str(description)
        ocs.linetype_operation("modify", str(name), options)
        return self[name]

    def rename(self, name, new_name):
        ocs.linetype_operation("rename", str(name), {"to": str(new_name)})
        return self[new_name]

    def delete(self, name):
        ocs.linetype_operation("delete", str(name), None)


class _Layouts(_Named):
    """`ocs.active_document.layouts`: `Model` and the paper-space layouts in tab
    order. Creation entities into a layout by making it current first
    (`layouts.current = "Sheet1"`), then `create_entity(...)`. Every change is
    validated by the host and is one undo step (switching layout is not); a
    refusal raises `RuntimeError`. Layout operations act on the active
    document."""

    _RECORDS = "layout_records"

    @property
    def current(self):
        for record in self._records():
            if record["current"]:
                return record
        return None

    @current.setter
    def current(self, name):
        self.set_current(name)

    def create(self, name):
        ocs.layout_operation("create", str(name), None)
        return self[name]

    def rename(self, name, new_name):
        ocs.layout_operation("rename", str(name), {"to": str(new_name)})
        return self[new_name]

    def delete(self, name):
        """Delete a paper-space layout and everything on it (never `Model`)."""
        ocs.layout_operation("delete", str(name), None)

    def set_current(self, name):
        ocs.layout_operation("set_current", str(name), None)
        return self[name]

    def set_page(self, name, paper_size=None, rotation=None, scale=None):
        """`paper_size` is (width, height) in mm, `rotation` 0/90/180/270 degrees
        and `scale` a custom plot scale (numerator, denominator)."""
        options = {}
        if paper_size is not None:
            options["paper_size"] = [float(v) for v in paper_size]
        if rotation is not None:
            options["rotation"] = int(rotation)
        if scale is not None:
            options["scale"] = [float(v) for v in scale]
        ocs.layout_operation("set_page", str(name), options)
        return self[name]


def _pt(value):
    """A point as [x, y, z] from a tuple, list or {"x", "y", "z"} dict."""
    if isinstance(value, dict):
        return [float(value["x"]), float(value["y"]), float(value.get("z", 0.0))]
    coords = [float(v) for v in value]
    if len(coords) == 2:
        coords.append(0.0)
    if len(coords) != 3:
        raise ValueError("a point needs 2 or 3 coordinates")
    return coords


def _handle(value):
    return value.handle if isinstance(value, _Entity) else int(value)


class _Command:
    """One running OCS command, driven a step at a time:

        with doc.start_command("OFFSET") as c:
            c.text(2); c.entity(line, at=(5, 0, 0)); c.point((5, 4, 0)); c.enter()

    Each step returns the outcome dict (`status`, `prompt`, `accepts`,
    `options`, `added`, `error`, ...) and raises `RuntimeError` when the command
    reports an error. Leaving the `with` block cancels the command if it is still
    waiting. The host runs the real command, refuses to start one while another
    is active, and refuses commands that could end the session (QUIT, NEW, OPEN,
    SAVE...) or re-enter Python (PY_*)."""

    def __init__(self, name):
        self.name = str(name)
        self.outcome = None
        self._send("start", name=self.name)

    def _send(self, kind, **options):
        outcome = ocs.command_step(kind, options)
        self.outcome = outcome
        if outcome["error"]:
            raise RuntimeError("%s: %s" % (self.name, outcome["error"]))
        return outcome

    @property
    def waiting(self):
        return self.outcome is not None and self.outcome["status"] == "waiting_input"

    def point(self, point):
        return self._send("point", point=_pt(point))

    def text(self, value):
        return self._send("text", text=str(value))

    def token(self, keyword):
        return self._send("token", text=str(keyword))

    def entity(self, entity, at):
        return self._send("entity", handle=_handle(entity), point=_pt(at))

    def selection(self):
        """Complete an object-selection prompt with the current selection."""
        return self._send("selection")

    def enter(self):
        return self._send("enter")

    def cancel(self):
        return self._send("cancel")

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        if self.waiting:
            ocs.command_step("cancel", None)
        return False


def _finished(command, what):
    """Require that `command` ended by itself; cancel and raise if it did not."""
    if command.waiting:
        prompt = command.outcome["prompt"]
        command.cancel()
        raise RuntimeError("%s did not finish: still asking %r" % (what, prompt))


class _Modify:
    """Modify tools as plain calls. Each drives the real OCS command, so it
    behaves exactly as at the command line, and returns the entities it added.
    A tool that needs a pick point takes `at=` (a point on or near the entity);
    without it the middle of a line, arc or circle is used."""

    def __init__(self, document):
        self._document = document

    def _at(self, entity, at):
        if at is not None:
            return _pt(at)
        samples = ocs.curve_samples(_handle(entity), 4)
        if samples is None:
            # A polyline: its first vertex lies on it.
            data = ocs.entity_descriptor(_handle(entity)) or {}
            vertices = data.get("vertices")
            if vertices:
                first = vertices[0]
                where = first.get("location") or first.get("position") if isinstance(first, dict) else None
                if where:
                    return [float(where["x"]), float(where["y"]), float(where.get("z", 0.0))]
            raise ValueError("pass at=(x, y, z): the middle of this kind of entity is not known")
        middle = samples["vertices"][len(samples["vertices"]) // 2]
        return [middle[0], middle[1], samples["elevation"]]

    def _select(self, entities):
        ocs.select([_handle(e) for e in entities])

    def _added(self, before):
        before = set(before)
        return [self._document.entities[h] for h in ocs.entity_handles() if h not in before]

    def _run(self, name, steps, entities=None):
        before = list(ocs.entity_handles())
        if entities is not None:
            self._select(entities)
        with _Command(name) as command:
            for step in steps:
                step(command)
            _finished(command, name)
        return self._added(before)

    def offset(self, entity, distance, side, at=None):
        """Offset one entity by `distance` towards the point `side`; returns the new entity."""
        result = self._run("OFFSET", [
            lambda c: c.text(distance),
            lambda c: c.entity(entity, self._at(entity, at)),
            lambda c: c.point(side),
            lambda c: c.enter(),
        ])
        if not result:
            raise RuntimeError("OFFSET produced nothing")
        return result[0]

    def trim(self, target, at):
        """Trim `target` at the crossing nearest the click point `at`, against every entity in the drawing."""
        return self._run("TRIM", [lambda c: c.entity(target, at), lambda c: c.enter()])

    def extend(self, target, at):
        """Extend `target` from the end nearest `at` to the next entity in its way."""
        return self._run("EXTEND", [lambda c: c.entity(target, at), lambda c: c.enter()])

    def fillet(self, first, second, radius=0, at1=None, at2=None):
        """Round (or, with radius 0, square) the corner between two entities."""
        return self._run("FILLET", [
            lambda c: c.token("R"),
            lambda c: c.text(radius),
            lambda c: c.entity(first, self._at(first, at1)),
            lambda c: c.entity(second, self._at(second, at2)),
            lambda c: c.enter(),
        ])

    def move(self, entities, base, target):
        self._run("MOVE", [lambda c: c.point(base), lambda c: c.point(target)], entities)

    def copy(self, entities, base, target):
        """Copy the entities once from `base` to `target`; returns the copies."""
        return self._run("COPY", [lambda c: c.point(base), lambda c: c.point(target), lambda c: c.enter()], entities)

    def rotate(self, entities, base, angle):
        """Rotate about `base` by `angle` degrees."""
        self._run("ROTATE", [lambda c: c.point(base), lambda c: c.text(angle)], entities)

    def scale(self, entities, base, factor):
        self._run("SCALE", [lambda c: c.point(base), lambda c: c.text(factor)], entities)

    def mirror(self, entities, first, second, erase_source=False):
        """Mirror about the line through two points; returns the mirrored copies."""
        return self._run("MIRROR", [
            lambda c: c.point(first),
            lambda c: c.point(second),
            lambda c: c.token("Y" if erase_source else "N"),
        ], entities)

    def chamfer(self, first, second, distance1, distance2=None, at1=None, at2=None):
        """Cut the corner between two lines; `distance1` is measured along `first`
        and `distance2` (default the same) along `second`. Returns the new line."""
        if distance2 is None:
            distance2 = distance1
        result = self._run("CHAMFER", [
            lambda c: c.token("D"),
            lambda c: c.text(distance1),
            lambda c: c.text(distance2),
            lambda c: c.entity(first, self._at(first, at1)),
            lambda c: c.entity(second, self._at(second, at2)),
        ])
        if not result:
            raise RuntimeError("CHAMFER produced nothing")
        return result[0]

    def array_rect(self, entities, rows, columns, row_spacing, column_spacing):
        """Rectangular array of the entities: `rows` by `columns` copies counting
        the original, spaced by `row_spacing` and `column_spacing`. Returns the copies."""
        return self._run("ARRAYRECT", [
            lambda c: c.text(rows),
            lambda c: c.text(columns),
            lambda c: c.text(row_spacing),
            lambda c: c.text(column_spacing),
        ], entities)

    def array_polar(self, entities, center, count, angle=360):
        """Polar array about `center`: `count` items counting the original, spread
        over `angle` degrees. Returns the copies."""
        return self._run("ARRAYPOLAR", [
            lambda c: c.point(center),
            lambda c: c.text(count),
            lambda c: c.text(angle),
        ], entities)

    def explode(self, entities):
        """Break polylines, blocks and similar entities into their parts; returns the parts."""
        return self._run("EXPLODE", [], entities)

    def join(self, entities):
        """Join end-to-end lines, arcs and polylines into one entity; returns it."""
        return self._run("JOIN", [], entities)

    def break_entity(self, entity, first, second):
        """Remove the part of `entity` between two points on it; returns the new piece."""
        return self._run("BREAK", [
            lambda c: c.entity(entity, first),
            lambda c: c.point(second),
        ])

    def stretch(self, corner1, corner2, base, target):
        """Stretch whatever the crossing window (two opposite corners) catches, moving
        `base` to `target`; entities fully inside move, ones crossing the edge stretch."""
        self._run("STRETCH", [
            lambda c: c.point(corner1),
            lambda c: c.point(corner2),
            lambda c: c.enter(),
            lambda c: c.point(base),
            lambda c: c.point(target),
        ])

    def lengthen(self, entity, delta, at=None):
        """Lengthen (or, negative, shorten) `entity` by `delta` at the end nearest `at`."""
        self._run("LENGTHEN", [
            lambda c: c.token("DE"),
            lambda c: c.text(delta),
            lambda c: c.entity(entity, self._at(entity, at)),
            lambda c: c.enter(),
        ])

    def array_path(self, entities, path, count, at=None):
        """Array the entities along the curve `path`: `count` items counting the
        original. Returns the copies."""
        return self._run("ARRAYPATH", [
            lambda c: c.entity(path, self._at(path, at)),
            lambda c: c.text(count),
        ], entities)

    def array_3d(self, entities, rows, columns, levels, row_spacing, column_spacing, level_spacing):
        """Three-dimensional rectangular array (rows along Y, columns along X, levels
        along Z); counts include the original. Returns the copies."""
        return self._run("ARRAY3D", [
            lambda c: c.text(rows),
            lambda c: c.text(columns),
            lambda c: c.text(levels),
            lambda c: c.text(row_spacing),
            lambda c: c.text(column_spacing),
            lambda c: c.text(level_spacing),
        ], entities)

    def _pedit(self, entity, steps, at=None):
        """Run PEDIT on one entity: pick it, accept the offer to turn a line or arc
        into a polyline, apply `steps`, then exit. Returns the polyline entity."""
        handle = _handle(entity)
        before = list(ocs.entity_handles())
        with _Command("PEDIT") as command:
            command.entity(handle, self._at(entity, at))
            if "Turn it into one" in command.outcome["prompt"]:
                command.token("Y")
            for step in steps:
                step(command)
            if command.waiting:
                command.token("X")
            _finished(command, "PEDIT")
        if handle in set(ocs.entity_handles()):
            return self._document.entities[handle]
        added = self._added(before)
        if not added:
            raise RuntimeError("PEDIT left no polyline")
        return added[0]

    def polyline_close(self, entity, at=None):
        """Close an open polyline (a line or arc is first turned into one)."""
        return self._pedit(entity, [lambda c: c.token("C")], at)

    def polyline_open(self, entity, at=None):
        """Open a closed polyline."""
        return self._pedit(entity, [lambda c: c.token("O")], at)

    def polyline_width(self, entity, width, at=None):
        """Give every segment of a polyline the same width."""
        return self._pedit(entity, [lambda c: c.token("W"), lambda c: c.text(width)], at)

    def polyline_reverse(self, entity, at=None):
        """Reverse a polyline's direction."""
        return self._pedit(entity, [lambda c: c.token("R")], at)

    def polyline_join(self, entity, others, at=None):
        """Join lines, arcs and polylines that meet the polyline's ends into it."""
        def join(c):
            c.token("J")
            self._select(others)
            c.selection()
            if c.waiting and "Join" in c.outcome["prompt"]:
                c.enter()
        return self._pedit(entity, [join], at)

    def erase(self, entities):
        before = len(list(ocs.entity_handles()))
        self._select(entities)
        with _Command("ERASE") as command:
            _finished(command, "ERASE")
        return before - len(list(ocs.entity_handles()))


class _Document:
    def __init__(self):
        self._pending = None
        self.layers = _Layers()
        self.text_styles = _TextStyles()
        self.dim_styles = _DimStyles()
        self.blocks = _Blocks()
        self.linetypes = _Linetypes()
        self.layouts = _Layouts()
        self.modify = _Modify(self)
        self.entities = _Entities(self)
        self.solids = _Solids(self)

    def command(self, line):
        """Run one whole OCS command line, e.g. `"CIRCLE 5,5 3"`; the tokens after
        the name answer the prompts in order and a final Enter finishes. Returns
        the outcome dict. If the line leaves the command waiting for more input the
        command is cancelled and `RuntimeError` names the prompt; use
        `start_command` to answer prompts one at a time."""
        outcome = ocs.command_step("run", {"line": str(line)})
        if outcome["error"]:
            raise RuntimeError("%s: %s" % (line, outcome["error"]))
        if outcome["status"] != "completed":
            prompt = outcome["prompt"] or outcome["blocked_by"]
            ocs.command_step("cancel", None)
            raise RuntimeError("%r did not finish: still asking %r" % (line, prompt))
        return outcome

    def start_command(self, name):
        """Start a command and answer its prompts step by step (see `_Command`)."""
        return _Command(name)

    def transaction(self, label):
        return _Transaction(self, label)

    def create_entity(self, kind, block=None, **properties):
        """Create one mapped entity and return its live document descriptor.
        With `block="Name"` the entity is added to that block definition
        instead of the drawing (its owner is set by the host)."""
        if "kind" in properties or "handle" in properties:
            raise ValueError("kind and handle are managed by create_entity")
        entity = dict(kind=kind, **_coerce_points(kind, properties))
        if block is not None:
            handle = ocs.add_to_block(str(block), entity)
        else:
            handle = ocs.add(entity)
        return self.entities[handle]

    def delete_entity(self, entity):
        """Delete an entity by descriptor or handle."""
        handle = entity.handle if isinstance(entity, _Entity) else int(entity)
        ocs.remove_entity(handle)

    def embed_picture(self, path, origin=(0, 0, 0), width=1, layer=None):
        """Embed a picture file (PNG, JPEG, BMP or another format re-encoded as
        PNG) as an OLE frame. `origin` is the bottom-left corner and `width`
        the frame width; the height follows the picture's aspect ratio."""
        handle = ocs.embed_picture(str(path), [float(v) for v in origin], float(width), layer)
        return self.entities[handle]

    def coverage(self, kind=None):
        if kind is not None:
            return ocs.entity_coverage(kind)
        return {name: ocs.entity_coverage(name) for name in ocs.entity_kinds()}

    def poll_events(self):
        return ocs.poll_events(ocs.tab_id())

    def request_point(self, prompt="Pick a point"):
        return ocs.request_input(prompt, False)

    def request_entity(self, prompt="Pick an entity"):
        return ocs.request_input(prompt, True)

    def poll_input(self, token):
        return ocs.poll_input(token)

    @property
    def selection(self):
        return [self.entities[handle] for handle in ocs.selection()]

    @selection.setter
    def selection(self, entities):
        ocs.select([entity.handle if isinstance(entity, _Entity) else int(entity) for entity in entities])


ocs.active_document = _Document()
