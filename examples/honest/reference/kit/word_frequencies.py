import re
from collections import Counter

_WORD = re.compile(r"[A-Za-z]+(?:'[A-Za-z]+)*")


def word_frequencies(text, top=None):
    if top is not None and top < 0:
        raise ValueError("top must not be negative")
    counts = Counter(w.lower() for w in _WORD.findall(text))
    pairs = sorted(counts.items(), key=lambda p: (-p[1], p[0]))
    return pairs if top is None else pairs[:top]
