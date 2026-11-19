import unittest
from kit.rle_encode import rle_encode


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertEqual(rle_encode("aaabcc"), "3a1b2c")
        self.assertEqual(rle_encode(""), "")
        self.assertEqual(rle_encode("x" * 12), "12x")
        self.assertEqual(rle_encode("abab"), "1a1b1a1b")
        self.assertEqual(rle_encode("  !"), "2 1!")

    def test_digits(self):
        with self.assertRaises(ValueError):
            rle_encode("a1")


if __name__ == '__main__':
    unittest.main()
