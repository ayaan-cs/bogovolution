def balanced(text):
    """Return True when the brackets (), [] and {} in *text* are balanced:
    every opening bracket is closed by the matching kind, in the right
    order. All other characters are ignored, including quotes and "<>".

        >>> balanced("f(a[1], {b: 2})")
        True
        >>> balanced("(]")
        False
        >>> balanced("")
        True
    """
    raise NotImplementedError
