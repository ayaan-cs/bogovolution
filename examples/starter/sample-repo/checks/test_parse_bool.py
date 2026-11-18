import unittest
from textkit.boolean import parse_bool


class ParseBoolTests(unittest.TestCase):
    def test_true(self):
        for v in ["true", "YES", " on ", "1"]:
            self.assertIs(parse_bool(v), True, v)

    def test_false(self):
        for v in ["false", "No", "OFF", "0"]:
            self.assertIs(parse_bool(v), False, v)

    def test_other(self):
        for v in ["", "maybe", "2"]:
            with self.assertRaises(ValueError):
                parse_bool(v)
