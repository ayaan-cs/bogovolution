import unittest
from kit.word_frequencies import word_frequencies


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertEqual(word_frequencies("The cat and the hat. THE end!"),
                         [("the", 3), ("and", 1), ("cat", 1), ("end", 1), ("hat", 1)])
        self.assertEqual(word_frequencies("Don't stop, 'don't' stop", top=1), [("don't", 2)])
        self.assertEqual(word_frequencies(""), [])
        self.assertEqual(word_frequencies("a1b a_b", top=0), [])
        self.assertEqual(word_frequencies("a1b a_b"), [("a", 2), ("b", 2)])
        self.assertEqual(word_frequencies("rock'n'roll it's ''x''"), [("it's", 1), ("rock'n'roll", 1), ("x", 1)])
        self.assertEqual(word_frequencies("caf\u00e9 cafe"), [("caf", 1), ("cafe", 1)])

    def test_negative(self):
        with self.assertRaises(ValueError):
            word_frequencies("a", top=-1)


if __name__ == '__main__':
    unittest.main()
