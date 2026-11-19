def normalize_path(path):
    absolute = path.startswith("/")
    parts = []
    for p in path.split("/"):
        if p in ("", "."):
            continue
        if p == "..":
            if parts and parts[-1] != "..":
                parts.pop()
            elif not absolute:
                parts.append("..")
            continue
        parts.append(p)
    body = "/".join(parts)
    if absolute:
        return "/" + body
    return body or "."
