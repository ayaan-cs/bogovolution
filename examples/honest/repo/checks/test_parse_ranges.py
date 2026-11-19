import unittest
from kit.parse_ranges import parse_ranges


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertEqual(parse_ranges("1-3,5,7-8"), [1, 2, 3, 5, 7, 8])
        self.assertEqual(parse_ranges(" 5 , 1-2, 2 "), [1, 2, 5])
        self.assertEqual(parse_ranges(""), [])
        self.assertEqual(parse_ranges("   "), [])
        self.assertEqual(parse_ranges("0"), [0])
        self.assertEqual(parse_ranges("4-4"), [4])

    def test_errors(self):
        for bad in ["5-3", "1-", "-2", "a", "1-2-3", "1 2", "1,,2", "1,"]:
            with self.assertRaises(ValueError, msg=bad):
                parse_ranges(bad)


if __name__ == '__main__':
    unittest.main()
