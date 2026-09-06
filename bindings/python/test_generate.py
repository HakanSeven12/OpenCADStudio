import inspect
import json
from pathlib import Path
import types
import sys
import unittest

from generate import render


class BindingTest(unittest.TestCase):
    def setUp(self):
        self.schema = json.loads((Path(__file__).resolve().parents[2] / 'src/gen/binding_schema.json').read_text())
        self.module = types.ModuleType('generated')
        sys.modules['generated'] = self.module
        self.addCleanup(sys.modules.pop, 'generated')
        exec(render(self.schema), self.module.__dict__)
        module = self.module

        class Transport:
            def __init__(self):
                self.calls = []

            def apply(self, envelope):
                self.calls.append(envelope)
                kind, payload = envelope.body
                if kind == 'Queries':
                    return module.Receipt(None, [{'Volume': 125.0}], 2)
                if 'CreateMany' in payload:
                    return module.Receipt({'NewIds': [10, 11]}, [], 2)
                return module.Receipt({'NewId': 9}, [], 2)

        self.transport = Transport()
        self.api = module.DocApi(self.transport, active_tab=4)
        self.doc = self.api.document(4)

    def test_constructors_returns_and_wire_payloads(self):
        solid = self.doc.solids().create_cuboid([0, 0, 0], [5, 5, 5])
        self.assertIsInstance(solid, self.module.Solid)
        self.assertEqual(self.transport.calls[-1].body, ('Op', {'CreateSolid': {'Cuboid': {'origin': [0, 0, 0], 'size': [5, 5, 5]}}}))
        self.assertEqual(solid.volume(), 125.0)
        self.assertIsInstance(solid.intersect(solid), self.module.Solid)
        self.assertEqual(self.transport.calls[-1].body[1], {'SolidBoolean': {'a': 9, 'b': 9, 'op': 'Intersection', 'erase_sources': True}})
        points = self.doc.curves().create_points([[0, 0, 0], [1, 1, 1]])
        self.assertEqual([p.id for p in points], [10, 11])
        self.assertTrue(all(isinstance(p, self.module.Point) for p in points))
        self.doc.solids().revolve(points[0], [0, 0, 0], [0, 0, 1], 3.14)
        self.assertEqual(self.transport.calls[-1].body[1]['Revolve']['axis'], ([0, 0, 0], [0, 0, 1]))
        self.doc.solids().loft(points)
        self.assertEqual(self.transport.calls[-1].body[1], {'Loft': {'profiles': [10, 11]}})
        self.assertIsNone(points[0].delete())

    def test_every_schema_method_and_constructor_survives_generation(self):
        for family in self.schema['families']:
            handle = getattr(self.module, family['handle'])
            collection = getattr(self.doc, family['collection'])()
            for method in family['methods']:
                self.assertTrue(callable(getattr(handle, method['name'])))
            for constructor in family['constructors']:
                method = getattr(collection, constructor['name'])
                self.assertEqual(list(inspect.signature(method).parameters), [a['name'] for a in constructor['args']])

    def test_foreign_handles_and_tab_mismatch_do_not_send(self):
        with self.assertRaises(ValueError):
            self.api.document(5)
        foreign = self.module.Solid(9, self.module.DocApi(object(), 4).document(4))
        local = self.module.Solid(9, self.doc)
        for operation in [lambda: local.union(foreign), lambda: self.doc.solids().extrude(foreign, [0, 0, 1]), lambda: self.doc.solids().loft([foreign])]:
            with self.assertRaises(ValueError):
                operation()
        self.assertEqual(self.transport.calls, [])


if __name__ == '__main__':
    unittest.main()
