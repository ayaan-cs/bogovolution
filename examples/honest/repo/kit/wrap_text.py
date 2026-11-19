def wrap_text(text, width):
    """Greedy word wrap. Return a list of lines, each at most *width*
    characters where possible.

    Words are split on any run of whitespace (spaces, tabs, newlines).
    Words on a line are joined by one space. Each line takes as many words
    as fit. A word longer than *width* goes alone on its own line, unbroken.
    Empty or whitespace only text gives [].

        >>> wrap_text("the quick brown fox", 10)
        ['the quick', 'brown fox']
        >>> wrap_text("a verylongword b", 4)
        ['a', 'verylongword', 'b']

    Raise ValueError when *width* is less than 1.
    """
    raise NotImplementedError
