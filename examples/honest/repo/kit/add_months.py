def add_months(day, months):
    """Add a whole number of months to the datetime.date *day*.

    *months* may be negative or zero. When the target month is shorter, the
    day of the month is clamped to its last day, so January 31 plus one
    month is February 28, or February 29 in a leap year. Return a new date.

        >>> import datetime
        >>> add_months(datetime.date(2024, 1, 31), 1)
        datetime.date(2024, 2, 29)
        >>> add_months(datetime.date(2024, 3, 15), -3)
        datetime.date(2023, 12, 15)

    Raise TypeError when *months* is not an int; a bool counts as not an
    int.
    """
    raise NotImplementedError
