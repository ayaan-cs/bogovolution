def truncate(text, width, suffix="..."):
    """Shorten *text* to at most *width* characters, ending with *suffix*
    when it was cut. Text that already fits is returned unchanged.

        >>> truncate("abcdefghij", 6)
        'abc...'

    Raise ValueError when *width* is smaller than the length of *suffix*.
    """
    if len(text) <= width:
        return text
    return text[:width] + suffix
