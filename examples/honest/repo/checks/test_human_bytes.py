import unittest
from kit.human_bytes import human_bytes


class Test(unittest.TestCase):
    def test_examples(self):
        for n, s in [(0, "0 B"), (512, "512 B"), (1023, "1023 B"), (1024, "1.0 KiB"),
                     (1536, "1.5 KiB"), (1048576, "1.0 MiB"), (1048575, "1024.0 KiB"),
                     (5 * 1024 ** 3, "5.0 GiB"), (1024 ** 5, "1024.0 TiB")]:
            self.assertEqual(human_bytes(n), s, n)

    def test_errors(self):
        with self.assertRaises(ValueError):
            human_bytes(-1)
        for bad in [1.5, "10", True]:
            with self.assertRaises(TypeError):
                human_bytes(bad)


if __name__ == '__main__':
    unittest.main()
