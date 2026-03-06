"""
The `cache` subpackage provides common caching infrastructure.

A running Nautilus system generally uses a single centralized cache which can be
accessed by many components.

"""

from nautilus_trader.cache.cache import Cache
from nautilus_trader.cache.database import CacheDatabaseAdapter


__all__ = [
    "Cache",
    "CacheDatabaseAdapter",
]
