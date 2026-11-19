def human_bytes(n):
    if isinstance(n, bool) or not isinstance(n, int):
        raise TypeError("n must be an int")
    if n < 0:
        raise ValueError("n must not be negative")
    if n < 1024:
        return f"{n} B"
    value = n / 1024
    for unit in ["KiB", "MiB", "GiB", "TiB"]:
        if value < 1024 or unit == "TiB":
            return f"{format(value, '.1f')} {unit}"
        value /= 1024
