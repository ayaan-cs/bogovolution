def word_frequencies(text, top=None):
    """Count words in *text*.

    A word is a run of ASCII letters, which may contain apostrophes between
    letters ("don't" is one word). Apostrophes at the start or end of a run
    are not part of the word. Words are lowercased. Everything else (digits,
    punctuation, other letters) separates words.

    Return a list of (word, count) pairs, highest count first and ties in
    alphabetical order. With *top* given, return only the first *top* pairs.

        >>> word_frequencies("The cat and the hat. THE end!")
        [('the', 3), ('and', 1), ('cat', 1), ('end', 1), ('hat', 1)]
        >>> word_frequencies("Don't stop, 'don't' stop", top=1)
        [("don't", 2)]

    Raise ValueError when *top* is negative.
    """
    raise NotImplementedError
