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

from __future__ import annotations

from nautilus_trader.flux.bridge.handlers.alerts import transform_alert
from nautilus_trader.flux.bridge.handlers.balances import transform_balances
from nautilus_trader.flux.bridge.handlers.events import transform_event
from nautilus_trader.flux.bridge.handlers.fv import transform_fv
from nautilus_trader.flux.bridge.handlers.market_bbo import transform_market_bbo
from nautilus_trader.flux.bridge.handlers.state import transform_state
from nautilus_trader.flux.bridge.handlers.trades import transform_trade
from nautilus_trader.flux.bridge.handlers.types import HandlerFn


def default_topic_handlers() -> dict[str, HandlerFn]:
    return {
        "state": transform_state,
        "event": transform_event,
        "trade": transform_trade,
        "alert": transform_alert,
        "market_bbo": transform_market_bbo,
        "fv": transform_fv,
        "balances": transform_balances,
    }


__all__ = ["default_topic_handlers"]
