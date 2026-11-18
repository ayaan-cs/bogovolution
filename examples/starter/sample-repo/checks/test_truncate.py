import unittest
from textkit.truncate import truncate


class TruncateTests(unittest.TestCase):
    def test_cut(self):
        self.assertEqual(truncate("abcdefghij", 6), "abc...")
        self.assertEqual(len(truncate("abcdefghij", 6)), 6)

    def test_fits(self):
        self.assertEqual(truncate("abc", 6), "abc")

    def test_custom_suffix(self):
        self.assertEqual(truncate("abcdefghij", 5, suffix="~"), "abcd~")

    def test_too_narrow(self):
        with self.assertRaises(ValueError):
            truncate("abcdefghij", 2)
