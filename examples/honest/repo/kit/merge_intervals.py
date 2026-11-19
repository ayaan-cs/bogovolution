def merge_intervals(intervals):
    """Merge overlapping (start, end) pairs.

    *intervals* is a list of (start, end) pairs of numbers with start <= end,
    in any order. Return a new list of (start, end) tuples sorted by start,
    where overlapping pairs are merged. Pairs that only touch (one ends
    where the next starts) are merged too. The input list is not modified.

        >>> merge_intervals([(5, 7), (1, 3), (2, 4)])
        [(1, 4), (5, 7)]
        >>> merge_intervals([(1, 2), (2, 3)])
        [(1, 3)]
        >>> merge_intervals([])
        []

    Raise ValueError when a pair has start greater than end.
    """
    raise NotImplementedError
