def csv_split(line):
    '''Split one CSV line into its fields.

    Fields are separated by commas. A field may be wrapped in double
    quotes; inside quotes a comma is an ordinary character and two double
    quotes ("") stand for one. The wrapping quotes are removed. Unquoted
    fields are returned exactly as written, spaces included. An empty line
    is one empty field.

        >>> csv_split('a,"b,c",d')
        ['a', 'b,c', 'd']
        >>> csv_split('"say ""hi""",x')
        ['say "hi"', 'x']
        >>> csv_split('')
        ['']
        >>> csv_split('a,,b ')
        ['a', '', 'b ']

    Raise ValueError for a quote that is never closed ('"abc'), a quote
    inside an unquoted field ('ab"c'), or anything other than a comma or the
    end of the line right after a closing quote ('"ab"c').
    '''
    raise NotImplementedError
