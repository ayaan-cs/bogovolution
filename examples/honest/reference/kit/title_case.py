_SMALL = {"a", "an", "the", "and", "or", "of", "in", "on", "to"}


def _cap(w):
    return w[:1].upper() + w[1:].lower()


def title_case(text):
    words = text.split()
    out = []
    for i, w in enumerate(words):
        if "-" in w:
            out.append("-".join(_cap(p) for p in w.split("-")))
        elif w.lower() in _SMALL and 0 < i < len(words) - 1:
            out.append(w.lower())
        else:
            out.append(_cap(w))
    return " ".join(out)
