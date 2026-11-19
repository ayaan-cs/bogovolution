def transpose(rows):
    if not rows:
        return []
    width = len(rows[0])
    if any(len(r) != width for r in rows):
        raise ValueError("rows have different lengths")
    return [[r[i] for r in rows] for i in range(width)]
