def flatten(nested, depth=None):
    """Flatten nested lists and tuples into one list.

    Only lists and tuples are opened. Strings, bytes, dicts, sets and every
    other object are kept as single items. With *depth* None everything is
    flattened; with depth n only n levels are opened, and depth 0 returns a
    plain list copy of the top level. The input is not modified.

        >>> flatten([1, [2, (3, [4])], "ab"])
        [1, 2, 3, 4, 'ab']
        >>> flatten([1, [2, [3, [4]]]], depth=1)
        [1, 2, [3, [4]]]

    Raise ValueError when *depth* is negative.
    """
    raise NotImplementedError
