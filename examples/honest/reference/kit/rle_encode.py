def rle_encode(text):
    if any(c.isdigit() for c in text):
        raise ValueError("text must not contain digits")
    out = []
    i = 0
    while i < len(text):
        j = i
        while j < len(text) and text[j] == text[i]:
            j += 1
        out.append(f"{j - i}{text[i]}")
        i = j
    return "".join(out)
