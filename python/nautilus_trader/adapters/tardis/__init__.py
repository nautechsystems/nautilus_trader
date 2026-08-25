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
Integration adapter for Tardis market data.
"""

from nautilus_trader._fixup import fixup_module_names
from nautilus_trader._libnautilus.tardis import *  # noqa: F403 (undefined-local-with-import-star)


__all__ = [
    "ReplayNormalizedRequestOptions",
    "StreamNormalizedRequestOptions",
    "TardisDataClientConfig",
    "TardisDataClientFactory",
    "convert_tardis_options_chain_csv",
    "load_tardis_deltas",
    "load_tardis_depth10_from_snapshot5",
    "load_tardis_depth10_from_snapshot25",
    "load_tardis_funding_rates",
    "load_tardis_options_chain",
    "load_tardis_quotes",
    "load_tardis_trades",
    "run_tardis_machine_replay",
    "stream_tardis_batched_deltas",
    "stream_tardis_deltas",
    "stream_tardis_depth10_from_snapshot5",
    "stream_tardis_depth10_from_snapshot25",
    "stream_tardis_funding_rates",
    "stream_tardis_options_chain",
    "stream_tardis_quotes",
    "stream_tardis_trades",
]

fixup_module_names(globals(), __name__)
del fixup_module_names
