import unittest
from kit.sliding_window import sliding_window


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertEqual(sliding_window([1, 2, 3, 4, 5], 3), [(1, 2, 3), (2, 3, 4), (3, 4, 5)])
        self.assertEqual(sliding_window("abcdef", 2, step=2), [("a", "b"), ("c", "d"), ("e", "f")])
        self.assertEqual(sliding_window([1, 2], 3), [])
        self.assertEqual(sliding_window([1, 2, 3, 4, 5], 2, step=3), [(1, 2), (4, 5)])
        self.assertEqual(sliding_window([1, 2, 3, 4], 3, step=2), [(1, 2, 3)])

    def test_errors(self):
        for n, step in [(0, 1), (-1, 1), (2, 0), (2, -1)]:
            with self.assertRaises(ValueError):
                sliding_window([1, 2, 3], n, step)


if __name__ == '__main__':
    unittest.main()
