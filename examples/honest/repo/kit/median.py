def median(values):
    """Return the median of a non-empty iterable of numbers.

    With an odd count the middle value of the sorted values is returned as
    it is. With an even count the result is the mean of the two middle
    values, (a + b) / 2. The input is not modified.

        >>> median([3, 1, 2])
        2
        >>> median([4, 1, 3, 2])
        2.5

    Raise ValueError when there are no values. Raise TypeError when a value
    is not an int or a float; a bool counts as neither.
    """
    raise NotImplementedError
