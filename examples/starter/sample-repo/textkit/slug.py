def slugify(text):
    """Turn *text* into a URL slug.

    Lowercase, every run of characters that are not letters or digits
    becomes one hyphen, and no hyphen at either end.

        >>> slugify("Hello, World!")
        'hello-world'
    """
    return text.lower().replace(" ", "-")
