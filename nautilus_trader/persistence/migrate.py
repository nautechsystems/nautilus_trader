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

from nautilus_trader.persistence.catalog.base import BaseDataCatalog
from nautilus_trader.persistence.external.core import write_objects


# TODO (bm)


def create_temp_table(func):
    """Make a temporary copy of any parquet dataset class called by `write_tables`"""

    def inner(*args, **kwargs):
        try:
            return func(*args, **kwargs)
        except Exception:
            # Restore old table
            print()

    return inner


write_objects = create_temp_table(write_objects)


def migrate(catalog: BaseDataCatalog, version_from: str, version_to: str):
    """Migrate the `catalog` between versions `version_from` and `version_to`"""
    pass
