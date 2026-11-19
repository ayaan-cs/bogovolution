_PAIRS = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"),
          (90, "XC"), (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"),
          (4, "IV"), (1, "I")]


def int_to_roman(n):
    if isinstance(n, bool) or not isinstance(n, int):
        raise TypeError("n must be an int")
    if not 1 <= n <= 3999:
        raise ValueError("n must be from 1 to 3999")
    out = []
    for value, letters in _PAIRS:
        while n >= value:
            out.append(letters)
            n -= value
    return "".join(out)
