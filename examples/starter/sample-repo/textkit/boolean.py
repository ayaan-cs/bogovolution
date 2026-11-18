def parse_bool(value):
    """Read a yes or no setting.

    "true", "yes", "on" and "1" are True; "false", "no", "off" and "0" are
    False. Case and surrounding spaces do not matter. Anything else raises
    ValueError.

        >>> parse_bool(" Yes ")
        True
    """
    raise NotImplementedError
