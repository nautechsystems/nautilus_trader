from __future__ import annotations

from nautilus_trader._libnautilus.model import *  # noqa: F403 (undefined-local-with-import-star)


try:  # pragma: no cover - optional extension may be absent
    from nautilus_trader._libnautilus.blockchain import Blockchain as _Blockchain
    from nautilus_trader._libnautilus.blockchain import Chain as _Chain
    from nautilus_trader._libnautilus.blockchain import Dex as _Dex  # type: ignore[attr-defined]
    from nautilus_trader._libnautilus.blockchain import DexType as _DexType
except ImportError:

    class _Blockchain:  # type: ignore[too-many-ancestors]
        ...

    class _Chain:  # type: ignore[too-many-ancestors]
        ...

    class _Dex:  # type: ignore[too-many-ancestors]
        ...

    class _DexType:  # type: ignore[too-many-ancestors]
        ...

else:
    Blockchain = _Blockchain
    Chain = _Chain
    Dex = _Dex
    DexType = _DexType


def _reassign_module_names() -> None:
    for _name, _obj in list(globals().items()):
        module = getattr(_obj, "__module__", "")
        if module.startswith("nautilus_trader.core.nautilus_pyo3.model"):
            try:
                _obj.__module__ = __name__
            except (AttributeError, TypeError):
                continue


_reassign_module_names()
del _reassign_module_names
