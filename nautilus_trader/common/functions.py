def one(iterable):
    it = iter(iterable)

    try:
        first_value = next(it)
    except StopIteration as e:
        raise (ValueError("too few items in iterable (expected 1)")) from e

    try:
        second_value = next(it)
    except StopIteration:
        pass
    else:
        msg = f"Expected exactly one item in iterable, but got {first_value}, {second_value}, and perhaps more."
        raise ValueError(msg)

    return first_value
