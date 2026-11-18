import unittest
from textkit.count import word_count


class WordCountTests(unittest.TestCase):
    def test_spaces(self):
        self.assertEqual(word_count("one two  three"), 3)

    def test_tabs_and_newlines(self):
        self.assertEqual(word_count("a\tb\nc "), 3)

    def test_empty(self):
        self.assertEqual(word_count(""), 0)
        self.assertEqual(word_count("   "), 0)
