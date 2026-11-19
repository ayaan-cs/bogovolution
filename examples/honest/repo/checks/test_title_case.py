import unittest
from kit.title_case import title_case


class Test(unittest.TestCase):
    def test_examples(self):
        for a, b in [("the lord OF the rings", "The Lord of the Rings"),
                     ("  a well-known   tale ", "A Well-Known Tale"),
                     ("what to", "What To"), ("", ""), ("of", "Of"),
                     ("war AND peace", "War and Peace"), ("up-to-date news", "Up-To-Date News"),
                     ("tHE eND", "The End")]:
            self.assertEqual(title_case(a), b, a)


if __name__ == '__main__':
    unittest.main()
