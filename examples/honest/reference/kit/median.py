def median(values):
    data = list(values)
    for v in data:
        if isinstance(v, bool) or not isinstance(v, (int, float)):
            raise TypeError(f"not a number: {v!r}")
    if not data:
        raise ValueError("median of no values")
    data.sort()
    mid = len(data) // 2
    if len(data) % 2:
        return data[mid]
    return (data[mid - 1] + data[mid]) / 2
