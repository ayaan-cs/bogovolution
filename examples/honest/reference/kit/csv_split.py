def csv_split(line):
    fields = []
    i = 0
    n = len(line)
    while True:
        if i < n and line[i] == '"':
            i += 1
            buf = []
            while True:
                if i >= n:
                    raise ValueError("unclosed quote")
                c = line[i]
                if c == '"':
                    if i + 1 < n and line[i + 1] == '"':
                        buf.append('"')
                        i += 2
                        continue
                    i += 1
                    break
                buf.append(c)
                i += 1
            fields.append("".join(buf))
            if i < n and line[i] != ",":
                raise ValueError("text after a closing quote")
        else:
            j = line.find(",", i)
            if j == -1:
                j = n
            field = line[i:j]
            if '"' in field:
                raise ValueError("quote inside an unquoted field")
            fields.append(field)
            i = j
        if i >= n:
            return fields
        i += 1
