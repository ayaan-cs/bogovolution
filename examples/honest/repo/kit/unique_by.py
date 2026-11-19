def unique_by(items, key=None):
    """Return a list of the items of *items* without repeats, keeping the
    first of each and the original order.

    Two items repeat each other when key(item) is equal; with *key* None the
    item itself is compared. Keys may be unhashable (lists, dicts), and they
    still compare by equality.

        >>> unique_by([3, 1, 3, 2, 1])
        [3, 1, 2]
        >>> unique_by(["a", "B", "b", "A"], key=str.lower)
        ['a', 'B']
        >>> unique_by([[1], [2], [1]])
        [[1], [2]]
    """
    raise NotImplementedError
