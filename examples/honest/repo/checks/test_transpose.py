import unittest
from kit.transpose import transpose


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertEqual(transpose([[1, 2, 3], [4, 5, 6]]), [[1, 4], [2, 5], [3, 6]])
        self.assertEqual(transpose([]), [])
        self.assertEqual(transpose([[1]]), [[1]])
        self.assertEqual(transpose([[1], [2]]), [[1, 2]])

    def test_ragged(self):
        for bad in [[[1, 2], [3]], [[1], [2, 3]], [[1, 2], [3, 4], []]]:
            with self.assertRaises(ValueError):
                transpose(bad)


if __name__ == '__main__':
    unittest.main()
