import unittest
from textkit.chunk import chunk


class ChunkTests(unittest.TestCase):
    def test_example(self):
        self.assertEqual(chunk([1, 2, 3, 4, 5], 2), [[1, 2], [3, 4], [5]])

    def test_empty(self):
        self.assertEqual(chunk([], 3), [])

    def test_bad_size(self):
        for size in [0, -1]:
            with self.assertRaises(ValueError):
                chunk([1, 2], size)
