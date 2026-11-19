def normalize_path(path):
    """Normalize a POSIX style path as text, without looking at the disk.

    Repeated slashes become one, "." parts are dropped, and ".." removes the
    part before it. A ".." that has nothing to remove is kept in a relative
    path ("../a") and dropped in an absolute one ("/.." is "/"). A trailing
    slash is removed except for the root "/". A path that ends up empty is
    ".".

        >>> normalize_path("a//b/./c/..")
        'a/b'
        >>> normalize_path("/../x/")
        '/x'
        >>> normalize_path("../../a/..")
        '../..'
        >>> normalize_path("")
        '.'
    """
    raise NotImplementedError
