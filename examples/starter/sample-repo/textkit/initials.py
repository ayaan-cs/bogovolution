def initials(name):
    """Return the upper case initials of *name*.

        >>> initials("ada  lovelace")
        'AL'
    """
    return "".join(part[0] for part in name.split(" ")).upper()
