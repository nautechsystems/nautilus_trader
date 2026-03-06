"""
NautilusTrader (http://nautilustrader.io) is an open-source, high-performance, production-grade
algorithmic trading platform, providing quantitative traders with the ability to backtest
portfolios of automated trading strategies on historical data with an event-driven engine,
and also deploy those same strategies live, with no code changes.
"""

from pathlib import Path
from typing import Final

from nautilus_trader.core import nautilus_pyo3


__version__ = nautilus_pyo3.NAUTILUS_VERSION

PACKAGE_ROOT: Final[Path] = Path(__file__).resolve().parent.parent
TEST_DATA_DIR: Final[Path] = PACKAGE_ROOT / "tests" / "test_data"

NAUTILUS_USER_AGENT: Final[str] = nautilus_pyo3.NAUTILUS_USER_AGENT
