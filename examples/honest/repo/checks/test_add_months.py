import unittest
import datetime
from kit.add_months import add_months


class Test(unittest.TestCase):
    def test_examples(self):
        D = datetime.date
        self.assertEqual(add_months(D(2024, 1, 31), 1), D(2024, 2, 29))
        self.assertEqual(add_months(D(2023, 1, 31), 1), D(2023, 2, 28))
        self.assertEqual(add_months(D(2024, 3, 15), -3), D(2023, 12, 15))
        self.assertEqual(add_months(D(2024, 5, 31), 0), D(2024, 5, 31))
        self.assertEqual(add_months(D(2023, 10, 31), 14), D(2024, 12, 31))
        self.assertEqual(add_months(D(2024, 3, 31), -1), D(2024, 2, 29))
        self.assertEqual(add_months(D(2024, 1, 1), -25), D(2021, 12, 1))

    def test_type(self):
        for bad in [1.0, "1", True]:
            with self.assertRaises(TypeError):
                add_months(datetime.date(2024, 1, 1), bad)


if __name__ == '__main__':
    unittest.main()
