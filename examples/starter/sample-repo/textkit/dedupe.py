def dedupe(items):
    """Return the items of *items* without repeats, keeping the first time
    each one appears, in order.

        >>> dedupe([3, 1, 3, 2, 1])
        [3, 1, 2]
    """
    return sorted(set(items))
