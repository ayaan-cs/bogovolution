def title_case(text):
    """Title case a heading.

    Words are split on any run of whitespace and joined by single spaces in
    the result, so leading and trailing whitespace disappears. Each word
    gets its first letter uppercased and the rest lowercased. These small
    words stay all lowercase unless they are the first or the last word:
    a, an, the, and, or, of, in, on, to. In a hyphenated word each part is
    treated like a word of its own, but the small word rule does not apply
    inside it.

        >>> title_case("the lord OF the rings")
        'The Lord of the Rings'
        >>> title_case("  a well-known   tale ")
        'A Well-Known Tale'
        >>> title_case("what to")
        'What To'
    """
    raise NotImplementedError
