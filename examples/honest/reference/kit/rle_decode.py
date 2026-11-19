def rle_decode(code):
    out = []
    i = 0
    while i < len(code):
        j = i
        while j < len(code) and code[j].isdigit():
            j += 1
        if j == i:
            raise ValueError(f"missing count at {i}")
        if j == len(code):
            raise ValueError("count with no character at the end")
        n = int(code[i:j])
        if n == 0:
            raise ValueError("count of 0")
        out.append(code[j] * n)
        i = j + 1
    return "".join(out)
