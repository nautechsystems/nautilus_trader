# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Dict, Optional

import pydantic

from nautilus_trader.persistence.catalog import DataCatalog


class PersistenceConfig(pydantic.BaseModel):
    """
    Configuration for persisting live or backtest runs to the catalog in feather format.

    Parameters
    ----------
    catalog_path : str
        The path to the data catalog
    fs_protocol : str
        The fsspec filesystem protocol of the catalog
    persist_logs: bool
        Persist log file to catalog
    flush_interval : int
        How often to write chunks, in milliseconds
    """

    catalog_path: str
    kind: str  # live or backtest
    fs_protocol: Optional[str] = None
    fs_storage_options: Optional[Dict] = None
    persist_logs: bool = False
    flush_interval: Optional[int] = None
    replace_existing: bool = False

    @classmethod
    def from_catalog(cls, catalog: DataCatalog, **kwargs):
        return cls(catalog_path=str(catalog.path), fs_protocol=catalog.fs.protocol, **kwargs)

    def as_catalog(self) -> DataCatalog:
        return DataCatalog(
            path=self.catalog_path,
            fs_protocol=self.fs_protocol,
            fs_storage_options=self.fs_storage_options,
        )
