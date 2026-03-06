from __future__ import annotations

from nautilus_trader._libnautilus.blockchain import *  # noqa: F403 (undefined-local-with-import-star)


def _reassign_module_names() -> None:
    for _name, _obj in list(globals().items()):
        module = getattr(_obj, "__module__", "")
        if module.startswith("nautilus_trader.core.nautilus_pyo3.blockchain"):
            try:
                _obj.__module__ = __name__
            except (AttributeError, TypeError):
                continue


_reassign_module_names()
del _reassign_module_names
