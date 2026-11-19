def camel_to_snake(name):
    out = []
    for i, c in enumerate(name):
        if c.isupper() and i > 0:
            prev = name[i - 1]
            nxt = name[i + 1] if i + 1 < len(name) else ""
            if prev.islower() or prev.isdigit() or (prev.isupper() and nxt.islower()):
                if out and out[-1] != "_":
                    out.append("_")
        out.append(c.lower())
    return "".join(out)
