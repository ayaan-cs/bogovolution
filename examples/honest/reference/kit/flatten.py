def flatten(nested, depth=None):
    if depth is not None and depth < 0:
        raise ValueError("depth must not be negative")
    out = []
    for item in nested:
        if isinstance(item, (list, tuple)) and (depth is None or depth > 0):
            out.extend(flatten(item, None if depth is None else depth - 1))
        else:
            out.append(item)
    return out
