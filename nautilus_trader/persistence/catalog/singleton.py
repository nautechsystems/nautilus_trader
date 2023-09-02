import inspect


class Singleton(type):
    """
    The base class to ensure a singleton.
    """

    def __init__(cls, name, bases, dict_like):
        super().__init__(name, bases, dict_like)
        cls._instances = {}

    def __call__(cls, *args, **kw):
        full_kwargs = resolve_kwargs(cls.__init__, None, *args, **kw)
        if full_kwargs == {"self": None, "args": (), "kwargs": {}}:
            full_kwargs = {}
        full_kwargs.pop("self", None)
        key = tuple(full_kwargs.items())
        if key not in cls._instances:
            cls._instances[key] = super().__call__(*args, **kw)
        return cls._instances[key]


def clear_singleton_instances(cls: type):
    assert isinstance(cls, Singleton)
    cls._instances = {}


def resolve_kwargs(func, *args, **kwargs):
    kw = inspect.getcallargs(func, *args, **kwargs)
    return {k: check_value(v) for k, v in kw.items()}


def check_value(v):
    if isinstance(v, dict):
        return freeze_dict(dict_like=v)
    return v


def freeze_dict(dict_like: dict):
    return tuple(sorted(dict_like.items()))
