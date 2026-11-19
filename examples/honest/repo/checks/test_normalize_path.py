import unittest
from kit.normalize_path import normalize_path


class Test(unittest.TestCase):
    def test_examples(self):
        for a, b in [("a//b/./c/..", "a/b"), ("/../x/", "/x"), ("../../a/..", "../.."),
                     ("", "."), ("/", "/"), ("//", "/"), (".", "."), ("a/..", "."),
                     ("./a/", "a"), ("/a/b/../../..", "/"), ("a/../../b", "../b"),
                     ("/a/./b//c/", "/a/b/c")]:
            self.assertEqual(normalize_path(a), b, a)


if __name__ == '__main__':
    unittest.main()
