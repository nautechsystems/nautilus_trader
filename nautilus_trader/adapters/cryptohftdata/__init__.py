# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
CryptoHFTData historical crypto market data integration adapter.

This adapter downloads CHD hourly ``.parquet.zst`` files through the Rust
backend and converts them into Nautilus data objects without depending on the
CHD Python SDK.
"""

from nautilus_trader.adapters.cryptohftdata.config import CryptoHFTDataCatalogIngestConfig
from nautilus_trader.adapters.cryptohftdata.config import CryptoHFTDataClientConfig
from nautilus_trader.adapters.cryptohftdata.constants import CRYPTOHFTDATA
from nautilus_trader.adapters.cryptohftdata.constants import CRYPTOHFTDATA_CLIENT_ID
from nautilus_trader.adapters.cryptohftdata.loaders import CryptoHFTDataDataLoader
from nautilus_trader.core.nautilus_pyo3 import CryptoHFTDataClient
from nautilus_trader.core.nautilus_pyo3 import CryptoHFTDataLiquidation
from nautilus_trader.core.nautilus_pyo3 import CryptoHFTDataOpenInterest
from nautilus_trader.core.nautilus_pyo3 import cryptohftdata_data_types
from nautilus_trader.core.nautilus_pyo3 import cryptohftdata_exchanges
from nautilus_trader.core.nautilus_pyo3 import run_cryptohftdata_ingest_from_config


__all__ = [
    "CRYPTOHFTDATA",
    "CRYPTOHFTDATA_CLIENT_ID",
    "CryptoHFTDataCatalogIngestConfig",
    "CryptoHFTDataClient",
    "CryptoHFTDataClientConfig",
    "CryptoHFTDataDataLoader",
    "CryptoHFTDataLiquidation",
    "CryptoHFTDataOpenInterest",
    "cryptohftdata_data_types",
    "cryptohftdata_exchanges",
    "run_cryptohftdata_ingest_from_config",
]
