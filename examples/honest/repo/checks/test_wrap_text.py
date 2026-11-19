import unittest
from kit.wrap_text import wrap_text


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertEqual(wrap_text("the quick brown fox", 10), ["the quick", "brown fox"])
        self.assertEqual(wrap_text("a verylongword b", 4), ["a", "verylongword", "b"])
        self.assertEqual(wrap_text("", 5), [])
        self.assertEqual(wrap_text(" \t\n ", 5), [])
        self.assertEqual(wrap_text("one\ttwo\nthree", 7), ["one two", "three"])
        self.assertEqual(wrap_text("ab cd", 5), ["ab cd"])

    def test_width(self):
        for bad in [0, -3]:
            with self.assertRaises(ValueError):
                wrap_text("a b", bad)


if __name__ == '__main__':
    unittest.main()
