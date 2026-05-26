# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

"""
NautilusTrader adapter for the LMEX cryptocurrency exchange.

Quick-start
-----------
.. code-block:: python

    from nautilus_trader.adapters.lmex import (
        LMEX_VENUE,
        LmexDataClientConfig,
        LmexExecClientConfig,
        LmexLiveDataClientFactory,
        LmexLiveExecClientFactory,
    )

    data_config = LmexDataClientConfig(
        api_key="YOUR_KEY",
        api_secret="YOUR_SECRET",
        is_sandbox=True,           # target test-api.lmex.io
    )

References
----------
- LMEX API docs: https://lmex.io/apidocs/spot/v3.2/
- NautilusTrader adapters guide: https://docs.nautilustrader.io/
"""

from nautilus_trader.adapters.lmex.config import LmexDataClientConfig
from nautilus_trader.adapters.lmex.config import LmexExecClientConfig
from nautilus_trader.adapters.lmex.constants import LMEX_VENUE
from nautilus_trader.adapters.lmex.data import LmexLiveMarketDataClient
from nautilus_trader.adapters.lmex.execution import LmexLiveExecutionClient
from nautilus_trader.adapters.lmex.factories import LmexLiveDataClientFactory
from nautilus_trader.adapters.lmex.factories import LmexLiveExecClientFactory
from nautilus_trader.adapters.lmex.providers import LmexInstrumentProvider


__all__ = [
    "LMEX_VENUE",
    "LmexDataClientConfig",
    "LmexExecClientConfig",
    "LmexInstrumentProvider",
    "LmexLiveMarketDataClient",
    "LmexLiveExecutionClient",
    "LmexLiveDataClientFactory",
    "LmexLiveExecClientFactory",
]
