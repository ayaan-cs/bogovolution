import unittest
from kit.parse_duration import parse_duration


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertEqual(parse_duration("1h30m"), 5400)
        self.assertEqual(parse_duration("45s"), 45)
        self.assertEqual(parse_duration("2h5s"), 7205)
        self.assertEqual(parse_duration("0s"), 0)
        self.assertEqual(parse_duration("1h2m3s"), 3723)

    def test_errors(self):
        for bad in ["", "5x", "h", "10", "1m1m", "1s1m", "1h 30m"]:
            with self.assertRaises(ValueError, msg=bad):
                parse_duration(bad)


if __name__ == '__main__':
    unittest.main()
