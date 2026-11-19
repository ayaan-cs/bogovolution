import unittest
from kit.isbn10_valid import isbn10_valid


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertTrue(isbn10_valid("0-306-40615-2"))
        self.assertTrue(isbn10_valid("0306406152"))
        self.assertTrue(isbn10_valid("080442957X"))
        self.assertTrue(isbn10_valid("0-8044-2957-X"))
        self.assertFalse(isbn10_valid("0306406153"))

    def test_invalid(self):
        for bad in ["080442957x", "08044X9570", "0306 406152", "030640615", "03064061522", "", "abcdefghij"]:
            self.assertFalse(isbn10_valid(bad), bad)


if __name__ == '__main__':
    unittest.main()
