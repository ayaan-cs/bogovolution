_PAIRS = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"),
          (90, "XC"), (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"),
          (4, "IV"), (1, "I")]


def _to_roman(n):
    out = []
    for value, letters in _PAIRS:
        while n >= value:
            out.append(letters)
            n -= value
    return "".join(out)


def roman_to_int(s):
    if not s or any(c not in "IVXLCDM" for c in s):
        raise ValueError(f"not a roman numeral: {s!r}")
    values = {"I": 1, "V": 5, "X": 10, "L": 50, "C": 100, "D": 500, "M": 1000}
    total = 0
    for i, c in enumerate(s):
        v = values[c]
        if i + 1 < len(s) and values[s[i + 1]] > v:
            total -= v
        else:
            total += v
    if not 1 <= total <= 3999 or _to_roman(total) != s:
        raise ValueError(f"not a standard roman numeral: {s!r}")
    return total
