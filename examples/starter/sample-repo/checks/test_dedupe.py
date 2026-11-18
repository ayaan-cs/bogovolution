import unittest
from textkit.dedupe import dedupe


class DedupeTests(unittest.TestCase):
    def test_order(self):
        self.assertEqual(dedupe([3, 1, 3, 2, 1]), [3, 1, 2])

    def test_strings(self):
        self.assertEqual(dedupe(["b", "a", "b"]), ["b", "a"])

    def test_empty(self):
        self.assertEqual(dedupe([]), [])
