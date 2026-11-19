import re

_ITEM = re.compile(r"(\d+)(?:-(\d+))?")


def parse_ranges(text):
    if not text.strip():
        return []
    out = set()
    for item in text.split(","):
        m = _ITEM.fullmatch(item.strip())
        if not m:
            raise ValueError(f"bad item: {item!r}")
        a = int(m.group(1))
        b = int(m.group(2)) if m.group(2) is not None else a
        if a > b:
            raise ValueError(f"range start after end: {item!r}")
        out.update(range(a, b + 1))
    return sorted(out)
