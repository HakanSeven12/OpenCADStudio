#!/usr/bin/env python3
"""Generate a Python facade over a caller-supplied document transport.

Usage: python generate.py ../../src/gen/binding_schema.json > ocs_doc.py
The transport converts these Python DTOs to/from the Rust wire representation.
"""
import json
import sys
from pathlib import Path

HEADER = '''"""Generated document facade. See bindings/README.md for the transport contract."""
from dataclasses import dataclass
from typing import Any

@dataclass
class DocApiEnvelope:
    version: int
    body: Any

@dataclass
class Receipt:
    outcome: Any
    query_results: list
    new_revision: int

@dataclass
class Entity:
    id: int
    _doc: "Document"

    def bounds(self):
        return self._doc._apply_query({"GetBounds": {"id": self.id}})

    def transform(self, placement):
        self._doc._apply_op({"Transform": {"id": self.id, "placement": placement}})

    def delete(self):
        self._doc._apply_op({"Delete": {"id": self.id}})
'''

DOCUMENT = '''
@dataclass
class Document:
    _transport: Any
    _tab: int

    def _id(self, value):
        if isinstance(value, Entity):
            if value._doc._transport is not self._transport or value._doc._tab != self._tab:
                raise ValueError("handle belongs to another document")
            return value.id
        return int(value)

    def _apply_op(self, op, want=None):
        receipt = self._transport.apply(DocApiEnvelope(ENVELOPE_VERSION, ("Op", op)))
        if want is None:
            return None
        if want.startswith("Vec<"):
            handle = globals()[want[4:-1]]
            return [handle(i, self) for i in receipt.outcome["NewIds"]]
        return globals()[want](receipt.outcome["NewId"], self)

    def _apply_query(self, query):
        receipt = self._transport.apply(DocApiEnvelope(ENVELOPE_VERSION, ("Queries", [query])))
        if len(receipt.query_results) != 1:
            raise ValueError("expected one query result")
        result = receipt.query_results[0]
        if not isinstance(result, dict) or len(result) != 1:
            raise ValueError("invalid query result")
        return next(iter(result.values()))

    def revision(self):
        return self._apply_query("GetGeometryRevision")

    def block_entities(self, block_name):
        return self._apply_query({"GetBlockEntities": {"block_name": block_name}})

    def solids(self): return Solids(self)
    def curves(self): return Curves(self)
    def entities(self): return Entities(self)

@dataclass
class DocApi:
    _transport: Any
    active_tab: int = 0

    def document(self, tab):
        if tab != self.active_tab:
            raise ValueError("transport is bound to another tab")
        return Document(self._transport, tab)
'''


def render_method(method, constructor=False):
    args = method.get("args", [])
    signature = "".join(", " + a["name"] for a in args)
    variant = method.get("op") or method["query"]
    fields = {}
    for arg in args:
        name, ty = arg["name"], arg["ty"]
        if ty in ("EntityRef", "Solid", "ObjectId"):
            fields[name] = f"self._doc._id({name})"
        elif ty == "&[EntityRef]":
            fields[name] = f"[self._doc._id(p) for p in {name}]"
        else:
            fields[name] = name
    fixed = dict(method.get("fixed") or {})
    if variant in ("SolidBoolean", "GetIntersects"):
        fields = {"a": "self.id", "b": fields.pop("other"), **fields}
        fields.pop("other", None)
    elif not constructor:
        fields = {"id": "self.id", **fields}
    if variant == "Revolve":
        fields["axis"] = "(pivot, axis)"
        fields.pop("pivot")
    nested = fixed.pop("primitive", fixed.pop("curve", None))
    fields.update({key: repr(value) for key, value in fixed.items()})
    payload = "{" + ", ".join(repr(k) + ": " + v for k, v in fields.items()) + "}"
    if variant == "CreateMany":
        payload = "[{'Curve': {'Point': {'position': p}}} for p in positions]"
    elif nested:
        payload = "{" + repr(nested) + ": " + payload + "}"
    message = "{" + repr(variant) + ": " + payload + "}"
    if method.get("kind") == "query":
        call = f"self._doc._apply_query({message})"
    else:
        want = method.get("returns")
        if want == "()":
            want = None
        call = f"self._doc._apply_op({message}, {want!r})"
    return f"    def {method['name']}(self{signature}):\n        return {call}\n"


def render(schema):
    out = [HEADER, f"ENVELOPE_VERSION = {schema['envelope_version']}\n"]
    handles = {name: {} for name in schema["object_model"]["handles"]}
    collections = {"solids": {}, "curves": {}, "entities": {}}
    for family in schema["families"]:
        handle = family.get("handle") or family["name"]
        for method in family.get("methods", []):
            existing = handles[handle].get(method["name"])
            if existing is not None and existing != method:
                raise ValueError(f"conflicting method: {handle}.{method['name']}")
            handles[handle][method["name"]] = method
        for constructor in family.get("constructors", []):
            collections[family["collection"]][constructor["name"]] = constructor
    # Families sharing Entity contribute to one class, preserving all methods.
    entity_methods = "\n".join(render_method(m) for m in handles.pop("Entity").values())
    out[0] += entity_methods
    for handle, methods in handles.items():
        out.append(f"\nclass {handle}(Entity):\n" + (
            "\n".join(render_method(m) for m in methods.values()) or "    pass\n"))
    for collection, constructors in collections.items():
        out.append(f"\nclass {collection.title()}:\n    def __init__(self, doc):\n        self._doc = doc\n")
        out.extend(render_method(c, constructor=True) for c in constructors.values())
    out.append(DOCUMENT)
    return "\n".join(out)


def main():
    default = Path(__file__).resolve().parents[2] / "src/gen/binding_schema.json"
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else default
    sys.stdout.write(render(json.loads(path.read_text(encoding="utf-8"))))


if __name__ == "__main__":
    main()
