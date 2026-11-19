def roman_to_int(s):
    """Convert an uppercase Roman numeral to an int (1 to 3999).

    Only the standard form is accepted: the one int_to_roman would write,
    using I, V, X, L, C, D, M with the subtractive pairs IV, IX, XL, XC, CD
    and CM.

        >>> roman_to_int("MCMXCIV")
        1994
        >>> roman_to_int("LVIII")
        58

    Raise ValueError for an empty string, lowercase letters or any other
    character, and for non standard forms such as "IIII", "VV", "IC", "IL"
    or "XM".
    """
    raise NotImplementedError
