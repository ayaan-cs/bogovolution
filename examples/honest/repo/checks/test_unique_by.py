import unittest
from kit.unique_by import unique_by


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertEqual(unique_by([3, 1, 3, 2, 1]), [3, 1, 2])
        self.assertEqual(unique_by(["a", "B", "b", "A"], key=str.lower), ["a", "B"])
        self.assertEqual(unique_by([[1], [2], [1]]), [[1], [2]])
        self.assertEqual(unique_by([]), [])
        self.assertEqual(unique_by(iter("abca")), ["a", "b", "c"])
        self.assertEqual(unique_by([{"k": 1}, {"k": 1}, 1, 1.0]), [{"k": 1}, 1])
        self.assertEqual(unique_by([(1, 2), (1, 3)], key=lambda p: [p[0]]), [(1, 2)])


if __name__ == '__main__':
    unittest.main()
