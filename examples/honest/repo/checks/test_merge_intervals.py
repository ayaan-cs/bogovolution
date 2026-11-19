import unittest
from kit.merge_intervals import merge_intervals


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertEqual(merge_intervals([(5, 7), (1, 3), (2, 4)]), [(1, 4), (5, 7)])
        self.assertEqual(merge_intervals([(1, 2), (2, 3)]), [(1, 3)])
        self.assertEqual(merge_intervals([]), [])
        self.assertEqual(merge_intervals([(1, 10), (2, 3)]), [(1, 10)])
        self.assertEqual(merge_intervals([[3, 4]]), [(3, 4)])

    def test_input_unchanged(self):
        data = [(5, 7), (1, 3)]
        merge_intervals(data)
        self.assertEqual(data, [(5, 7), (1, 3)])

    def test_bad_pair(self):
        with self.assertRaises(ValueError):
            merge_intervals([(1, 2), (5, 4)])


if __name__ == '__main__':
    unittest.main()
