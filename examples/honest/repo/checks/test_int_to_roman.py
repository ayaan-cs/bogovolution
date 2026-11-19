import unittest
from kit.int_to_roman import int_to_roman


class Test(unittest.TestCase):
    def test_examples(self):
        for n, s in [(1, "I"), (4, "IV"), (9, "IX"), (14, "XIV"), (40, "XL"),
                     (90, "XC"), (400, "CD"), (1994, "MCMXCIV"), (3999, "MMMCMXCIX")]:
            self.assertEqual(int_to_roman(n), s)

    def test_range(self):
        for bad in [0, -1, 4000]:
            with self.assertRaises(ValueError):
                int_to_roman(bad)

    def test_type(self):
        for bad in ["5", 2.0, True, None]:
            with self.assertRaises(TypeError):
                int_to_roman(bad)


if __name__ == '__main__':
    unittest.main()
