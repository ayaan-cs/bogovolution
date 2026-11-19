def _parts(v):
    if not v:
        raise ValueError("empty version")
    parts = v.split(".")
    for p in parts:
        if not p or not (p.isascii() and p.isdigit()):
            raise ValueError(f"bad version: {v!r}")
    return [int(p) for p in parts]


def compare_versions(a, b):
    x, y = _parts(a), _parts(b)
    n = max(len(x), len(y))
    x += [0] * (n - len(x))
    y += [0] * (n - len(y))
    return (x > y) - (x < y)
