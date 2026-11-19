def luhn_valid(number):
    """Check a card style number with the Luhn checksum.

    *number* is a string of digits. Single spaces between digit groups are
    allowed and ignored ("4539 3195 0343 6467"). From the rightmost digit,
    double every second digit, subtract 9 from any result over 9, and sum
    everything; the number is valid when the sum is divisible by 10.

        >>> luhn_valid("79927398713")
        True
        >>> luhn_valid("79927398710")
        False

    Return False (never raise) for fewer than 2 digits, any character other
    than digits and spaces, or leading, trailing or doubled spaces.
    """
    raise NotImplementedError
