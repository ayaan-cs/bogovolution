import unittest
from textkit.slug import slugify


class SlugifyTests(unittest.TestCase):
    def test_example(self):
        self.assertEqual(slugify("Hello, World!"), "hello-world")

    def test_runs_and_ends(self):
        self.assertEqual(slugify("  --Rust & Python 3--  "), "rust-python-3")

    def test_empty(self):
        self.assertEqual(slugify(""), "")
