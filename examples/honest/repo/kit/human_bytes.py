def human_bytes(n):
    """Format a byte count for people, with 1024 based units.

    Below 1024 the result is the number and "B": "512 B". Otherwise divide
    by 1024 until the value is below 1024 or the unit is TiB, and format the
    value with one decimal using format(value, ".1f"), then a space and the
    unit: KiB, MiB, GiB or TiB.

        >>> human_bytes(0)
        '0 B'
        >>> human_bytes(1536)
        '1.5 KiB'
        >>> human_bytes(1048576)
        '1.0 MiB'
        >>> human_bytes(1024 ** 5)
        '1024.0 TiB'

    Raise ValueError when *n* is negative. Raise TypeError when *n* is not
    an int; a bool counts as not an int.
    """
    raise NotImplementedError
