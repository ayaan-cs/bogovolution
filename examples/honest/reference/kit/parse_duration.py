import re

_PART = re.compile(r"(\d+)([hms])")
_ORDER = "hms"
_SECONDS = {"h": 3600, "m": 60, "s": 1}


def parse_duration(text):
    if not text:
        raise ValueError("empty duration")
    pos = 0
    total = 0
    last = -1
    while pos < len(text):
        m = _PART.match(text, pos)
        if not m:
            raise ValueError(f"bad duration: {text!r}")
        unit = _ORDER.index(m.group(2))
        if unit <= last:
            raise ValueError(f"units out of order or repeated: {text!r}")
        last = unit
        total += int(m.group(1)) * _SECONDS[m.group(2)]
        pos = m.end()
    return total
