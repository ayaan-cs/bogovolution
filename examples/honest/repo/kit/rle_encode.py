def rle_encode(text):
    """Run length encode *text*: each run of the same character becomes the
    run length followed by the character.

        >>> rle_encode("aaabcc")
        '3a1b2c'
        >>> rle_encode("")
        ''
        >>> rle_encode("x" * 12)
        '12x'

    Raise ValueError when *text* contains a digit, since the result could
    not be decoded.
    """
    raise NotImplementedError
