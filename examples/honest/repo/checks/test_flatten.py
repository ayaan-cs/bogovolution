import unittest
from kit.flatten import flatten


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertEqual(flatten([1, [2, (3, [4])], "ab"]), [1, 2, 3, 4, "ab"])
        self.assertEqual(flatten([1, [2, [3, [4]]]], depth=1), [1, 2, [3, [4]]])
        self.assertEqual(flatten([1, [2, [3]]], depth=0), [1, [2, [3]]])
        self.assertEqual(flatten([]), [])
        self.assertEqual(flatten([{"a": 1}, {2}, b"x", [[]]]), [{"a": 1}, {2}, b"x"])
        self.assertEqual(flatten((1, (2,))), [1, 2])

    def test_copy(self):
        inner = [2]
        data = [1, inner]
        out = flatten(data, depth=0)
        self.assertEqual(out, [1, [2]])
        out.append(3)
        self.assertEqual(data, [1, inner])

    def test_negative(self):
        with self.assertRaises(ValueError):
            flatten([1], depth=-1)


if __name__ == '__main__':
    unittest.main()
