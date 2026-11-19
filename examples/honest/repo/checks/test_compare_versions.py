import unittest
from kit.compare_versions import compare_versions


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertEqual(compare_versions("1.2.10", "1.2.9"), 1)
        self.assertEqual(compare_versions("1.2", "1.2.0"), 0)
        self.assertEqual(compare_versions("1.9", "1.10"), -1)
        self.assertEqual(compare_versions("2", "1.99.99"), 1)
        self.assertEqual(compare_versions("0.0.1", "0.0.1"), 0)
        self.assertEqual(compare_versions("1.0.0", "1"), 0)

    def test_errors(self):
        for a, b in [("", "1"), ("1..2", "1"), ("1", "1.a"), ("-1", "1"), ("1.2.", "1")]:
            with self.assertRaises(ValueError):
                compare_versions(a, b)


if __name__ == '__main__':
    unittest.main()
