def parse_ranges(text):
    """Expand a list of page numbers and ranges such as "1-3,5,7-8".

    Items are separated by commas; spaces around an item are ignored. An
    item is a non-negative whole number or two of them joined by "-" (both
    ends included). Return a sorted list of ints without duplicates.
    Empty or whitespace only text gives [].

        >>> parse_ranges("1-3,5,7-8")
        [1, 2, 3, 5, 7, 8]
        >>> parse_ranges(" 5 , 1-2, 2 ")
        [1, 2, 5]

    Raise ValueError for a range whose start is greater than its end
    ("5-3"), and for a malformed item such as "", "1-", "-2", "a", "1-2-3"
    or "1 2" (an empty item between commas counts as malformed).
    """
    raise NotImplementedError
