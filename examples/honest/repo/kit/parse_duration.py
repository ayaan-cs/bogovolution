def parse_duration(text):
    """Convert a duration such as "1h30m" to a number of seconds.

    The text is one or more parts. Each part is a non-negative whole number
    followed by a unit: h (hours), m (minutes) or s (seconds). Units must
    appear in the order h, m, s, each at most once. No spaces are allowed.

        >>> parse_duration("1h30m")
        5400
        >>> parse_duration("45s")
        45
        >>> parse_duration("2h5s")
        7205
        >>> parse_duration("0s")
        0

    Raise ValueError for an empty string, an unknown unit ("5x"), a unit
    without a number ("h"), a number without a unit ("10"), a repeated unit
    ("1m1m"), units out of order ("1s1m"), or any space ("1h 30m").
    """
    raise NotImplementedError
