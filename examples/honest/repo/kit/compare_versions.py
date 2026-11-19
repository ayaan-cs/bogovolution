def compare_versions(a, b):
    """Compare two dotted version strings such as "1.2.10".

    Each part is a non-negative whole number written with digits only.
    Parts compare as numbers, so "1.10" is newer than "1.9". Missing
    trailing parts count as 0, so "1.2" equals "1.2.0".

    Return -1 when *a* is older than *b*, 0 when they are equal and 1 when
    *a* is newer.

        >>> compare_versions("1.2.10", "1.2.9")
        1
        >>> compare_versions("1.2", "1.2.0")
        0

    Raise ValueError when either string is empty or has an empty part or a
    part that is not all digits, for example "1..2", "1.a", "-1" or "1.2.".
    """
    raise NotImplementedError
