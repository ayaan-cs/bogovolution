def camel_to_snake(name):
    """Turn a camelCase or PascalCase name into snake_case.

    Insert an underscore before an uppercase letter that follows a lowercase
    letter or a digit, and before the last capital of a run of capitals when
    a lowercase letter follows it. Then lowercase everything. Never produce
    two underscores in a row, and keep underscores already there.

        >>> camel_to_snake("parseHTTPResponse")
        'parse_http_response'
        >>> camel_to_snake("userID")
        'user_id'
        >>> camel_to_snake("HTMLParser")
        'html_parser'
        >>> camel_to_snake("version2Update")
        'version2_update'
        >>> camel_to_snake("my_Var")
        'my_var'
        >>> camel_to_snake("simple")
        'simple'
    """
    raise NotImplementedError
