import unittest
from kit.rle_decode import rle_decode


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertEqual(rle_decode("3a1b2c"), "aaabcc")
        self.assertEqual(rle_decode("12x"), "x" * 12)
        self.assertEqual(rle_decode(""), "")
        self.assertEqual(rle_decode("2 1!"), "  !")

    def test_errors(self):
        for bad in ["0a", "a", "2ab", "3a2", "12"]:
            with self.assertRaises(ValueError, msg=bad):
                rle_decode(bad)


if __name__ == '__main__':
    unittest.main()
