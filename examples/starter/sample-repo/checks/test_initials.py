import unittest
from textkit.initials import initials


class InitialsTests(unittest.TestCase):
    def test_example(self):
        self.assertEqual(initials("ada  lovelace"), "AL")

    def test_spaces_around(self):
        self.assertEqual(initials("  grace brewster hopper "), "GBH")

    def test_empty(self):
        self.assertEqual(initials(""), "")
