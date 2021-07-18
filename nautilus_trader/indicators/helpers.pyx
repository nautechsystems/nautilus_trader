cdef bint cross_over(str direction, list values1, list values2) except *:
    """
    Determines if the values in list1 crossed list2 in the direction specirfied.

    Parameters
    ----------
    direction : str
        The direction of the cross over to check. Accepts 'UP' or 'DOWN'.
    values1 : list
        The first list of values.
    values2 : list
        The second list of values.

    Returns
    -------
    bint

    """
    if direction == "UP":
        return cross_up(values1, values2)
    elif direction == "DOWN":
        return cross_down(values1, values2)
    else:
        raise Exception("Direction can only be 'UP' or 'DOWN'")


cdef bint cross_up(list values1, list values2) except *:
    """
    Determines if the values in list1 crossed above list2.

    Parameters
    ----------
    values1 : list
        The first list of values.
    values2 : list
        The second list of values.

    Returns
    -------
    bint

    """
    return values1[-2] < values2[-2] and values1[-1] > values2[-1]


cdef bint cross_down(list values1, list values2) except *:
    """
    Determines if the values in list1 crossed below list2.

    Parameters
    ----------
    values1 : list
        The first list of values.
    values2 : list
        The second list of values.

    Returns
    -------
    bint

    """
    return values1[-2] > values2[-2] and values1[-1] < values2[-1]
