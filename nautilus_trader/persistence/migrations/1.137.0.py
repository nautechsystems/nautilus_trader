# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import warnings

import fsspec
import pyarrow.dataset as ds

from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.external.core import write_objects
from nautilus_trader.serialization.arrow.util import class_to_filename


FROM = "1.136.0"
TO = "1.137.0"


def main(catalog: ParquetDataCatalog):
    """Rename local_symbol to native_symbol in Instruments"""
    fs: fsspec.AbstractFileSystem = catalog.fs
    for cls in Instrument.__subclasses__():
        # Load instruments into memory
        instruments = catalog.instruments(
            as_nautilus=True,
            instrument_type=cls,
            projections={"native_symbol": ds.field("local_symbol")},
        )

        # Create temp parquet in case of error
        fs.move(
            f"{catalog.path}/data/{class_to_filename(cls)}.parquet",
            f"{catalog.path}/data/{class_to_filename(cls)}.parquet_tmp",
            recursive=True,
        )

        try:
            # Rewrite new instruments
            write_objects(catalog, instruments)

            # Ensure we can query again
            _ = catalog.instruments(instrument_type=cls, as_nautilus=True)

            # Clear temp parquet
            fs.rm(f"{catalog.path}/data/{class_to_filename(cls)}.parquet_tmp", recursive=True)
        except Exception:
            warnings.warn(f"Failed to write or read instrument type {cls}")
            fs.move(
                f"{catalog.path}/data/{class_to_filename(cls)}.parquet_tmp",
                f"{catalog.path}/data/{class_to_filename(cls)}.parquet",
                recursive=True,
            )
