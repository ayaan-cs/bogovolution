import calendar
import datetime


def add_months(day, months):
    if isinstance(months, bool) or not isinstance(months, int):
        raise TypeError("months must be an int")
    index = day.year * 12 + (day.month - 1) + months
    year, month = divmod(index, 12)
    month += 1
    last = calendar.monthrange(year, month)[1]
    return datetime.date(year, month, min(day.day, last))
