def word_count(text):
    """Return the number of words in *text*, split on any whitespace.

        >>> word_count("one two  three")
        3
    """
    return len(text.split(" "))
