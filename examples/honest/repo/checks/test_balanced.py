import unittest
from kit.balanced import balanced


class Test(unittest.TestCase):
    def test_examples(self):
        self.assertTrue(balanced("f(a[1], {b: 2})"))
        self.assertFalse(balanced("(]"))
        self.assertTrue(balanced(""))
        self.assertFalse(balanced("(("))
        self.assertFalse(balanced("))(("))
        self.assertTrue(balanced('<("x")>'))
        self.assertFalse(balanced("([)]"))
        self.assertFalse(balanced("}"))


if __name__ == '__main__':
    unittest.main()
