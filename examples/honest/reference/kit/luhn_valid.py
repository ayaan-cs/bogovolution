def luhn_valid(number):
    if not isinstance(number, str) or number != number.strip() or "  " in number:
        return False
    digits = number.replace(" ", "")
    if len(digits) < 2 or not (digits.isascii() and digits.isdigit()):
        return False
    total = 0
    for i, c in enumerate(reversed(digits)):
        d = int(c)
        if i % 2 == 1:
            d *= 2
            if d > 9:
                d -= 9
        total += d
    return total % 10 == 0
