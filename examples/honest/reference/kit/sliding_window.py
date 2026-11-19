def sliding_window(seq, n, step=1):
    if n < 1 or step < 1:
        raise ValueError("n and step must be at least 1")
    return [tuple(seq[i:i + n]) for i in range(0, len(seq) - n + 1, step)]
