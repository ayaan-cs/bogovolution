import unittest
from kit.csv_split import csv_split


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertEqual(csv_split('a,"b,c",d'), ["a", "b,c", "d"])
        self.assertEqual(csv_split('"say ""hi""",x'), ['say "hi"', "x"])
        self.assertEqual(csv_split(""), [""])
        self.assertEqual(csv_split("a,,b "), ["a", "", "b "])
        self.assertEqual(csv_split("a,"), ["a", ""])
        self.assertEqual(csv_split('""'), [""])
        self.assertEqual(csv_split('x,""'), ["x", ""])
        self.assertEqual(csv_split(' a , b'), [" a ", " b"])

    def test_errors(self):
        for bad in ['"abc', 'ab"c', '"ab"c', 'x,"y', '"a" ,b']:
            with self.assertRaises(ValueError, msg=bad):
                csv_split(bad)


if __name__ == '__main__':
    unittest.main()
