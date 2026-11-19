import unittest
from kit.median import median


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertEqual(median([3, 1, 2]), 2)
        self.assertEqual(median([4, 1, 3, 2]), 2.5)
        self.assertEqual(median([5]), 5)
        self.assertEqual(median(iter([1.5, 0.5])), 1.0)
        self.assertEqual(median((10, 2, 38, 23, 38, 23, 21)), 23)

    def test_unchanged(self):
        data = [3, 1, 2]
        median(data)
        self.assertEqual(data, [3, 1, 2])

    def test_errors(self):
        with self.assertRaises(ValueError):
            median([])
        for bad in [[1, "2"], [True, 2], [None]]:
            with self.assertRaises(TypeError):
                median(bad)


if __name__ == '__main__':
    unittest.main()
