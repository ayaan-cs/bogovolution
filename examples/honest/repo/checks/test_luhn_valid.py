import unittest
from kit.luhn_valid import luhn_valid


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertTrue(luhn_valid("79927398713"))
        self.assertFalse(luhn_valid("79927398710"))
        self.assertTrue(luhn_valid("4539 3195 0343 6467"))
        self.assertTrue(luhn_valid("059"))
        self.assertTrue(luhn_valid("00"))

    def test_invalid_input(self):
        for bad in ["0", "", " 059", "059 ", "05  9", "05a9", "8273-1232", "-059"]:
            self.assertFalse(luhn_valid(bad), bad)


if __name__ == '__main__':
    unittest.main()
