import unittest
from kit.roman_to_int import roman_to_int


class Test(unittest.TestCase):
    def test_examples(self):
        for s, n in [("I", 1), ("III", 3), ("IV", 4), ("IX", 9), ("LVIII", 58),
                     ("XL", 40), ("CD", 400), ("MCMXCIV", 1994), ("MMMCMXCIX", 3999)]:
            self.assertEqual(roman_to_int(s), n, s)

    def test_errors(self):
        for bad in ["", "iv", "IIII", "VV", "IC", "IL", "XM", "ABC", "MMMM"]:
            with self.assertRaises(ValueError, msg=bad):
                roman_to_int(bad)


if __name__ == '__main__':
    unittest.main()
