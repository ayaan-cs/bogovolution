import unittest
from kit.camel_to_snake import camel_to_snake


class Test(unittest.TestCase):
    def test_examples(self):
        for a, b in [("parseHTTPResponse", "parse_http_response"), ("userID", "user_id"),
                     ("HTMLParser", "html_parser"), ("version2Update", "version2_update"),
                     ("my_Var", "my_var"), ("simple", "simple"), ("already_snake", "already_snake"),
                     ("A", "a"), ("getX", "get_x"), ("IOError", "io_error"), ("", "")]:
            self.assertEqual(camel_to_snake(a), b, a)


if __name__ == '__main__':
    unittest.main()
