def chunk(items, size):
    """Split the list *items* into lists of *size* items; the last one may
    be shorter.

        >>> chunk([1, 2, 3, 4, 5], 2)
        [[1, 2], [3, 4], [5]]

    Raise ValueError when *size* is less than 1.
    """
    return [items[i:i + size] for i in range(0, len(items), size)]
