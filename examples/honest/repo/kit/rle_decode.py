def rle_decode(code):
    """Decode the output of a run length encoder: a count followed by one
    character, repeated.

        >>> rle_decode("3a1b2c")
        'aaabcc'
        >>> rle_decode("12x")
        'xxxxxxxxxxxx'
        >>> rle_decode("")
        ''

    A count has one or more digits and the character after it is never a
    digit. Raise ValueError for a count of 0 ("0a"), a character with no
    count ("a" or "2ab"), or digits at the end with no character ("3a2").
    """
    raise NotImplementedError
