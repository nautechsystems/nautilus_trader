# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.core import nautilus_pyo3


# Run the following to start the tardis-machine server:
# docker run -p 8000:8000 -p 8001:8001 -e "TM_API_KEY=YOUR_API_KEY" -d tardisdev/tardis-machine
#
# Export RUST_LOG=debug to see logging


async def run():
    config_filepath = (
        PACKAGE_ROOT
        / "nautilus_core"
        / "adapters"
        / "src"
        / "tardis"
        / "bin"
        / "example_config.json"
    )
    await nautilus_pyo3.run_tardis_machine_replay(str(config_filepath.resolve()))


if __name__ == "__main__":
    asyncio.run(run())
