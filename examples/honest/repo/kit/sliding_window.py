def sliding_window(seq, n, step=1):
    """Return the windows of *n* consecutive items of the sequence *seq* as a
    list of tuples, starting at index 0 and moving *step* items each time.
    Only full windows are returned, so a sequence shorter than *n* gives [].

        >>> sliding_window([1, 2, 3, 4, 5], 3)
        [(1, 2, 3), (2, 3, 4), (3, 4, 5)]
        >>> sliding_window("abcdef", 2, step=2)
        [('a', 'b'), ('c', 'd'), ('e', 'f')]

    Raise ValueError when *n* or *step* is less than 1.
    """
    raise NotImplementedError
