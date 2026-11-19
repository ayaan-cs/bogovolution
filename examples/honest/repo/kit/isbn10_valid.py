def isbn10_valid(text):
    """Check an ISBN-10.

    Hyphens are removed first, wherever they are. What is left must be
    exactly 10 characters: nine digits and then a digit or an uppercase "X"
    (worth 10). Multiply the characters by 10, 9, 8, ... 1 from left to
    right; the ISBN is valid when the sum is divisible by 11.

        >>> isbn10_valid("0-306-40615-2")
        True
        >>> isbn10_valid("080442957X")
        True
        >>> isbn10_valid("0306406153")
        False

    Return False (never raise) for any other input, including a lowercase
    "x", an "X" before the last place, spaces, or the wrong length.
    """
    raise NotImplementedError
