def isbn10_valid(text):
    if not isinstance(text, str):
        return False
    s = text.replace("-", "")
    if len(s) != 10:
        return False
    total = 0
    for i, c in enumerate(s):
        if c.isascii() and c.isdigit():
            v = int(c)
        elif c == "X" and i == 9:
            v = 10
        else:
            return False
        total += (10 - i) * v
    return total % 11 == 0
