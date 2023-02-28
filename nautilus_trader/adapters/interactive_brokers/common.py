# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import PRICE_MAX
from nautilus_trader.model.objects import Price
from nautilus_trader.model.tick_scheme.base import register_tick_scheme
from nautilus_trader.model.tick_scheme.implementations.fixed import FixedTickScheme


IB_VENUE = Venue("InteractiveBrokers")


class ContractId(int):
    """
    ContractId type.
    """


# https://interactivebrokers.github.io/tws-api/tick_types.html
TickTypeMapping = {
    0: "Bid Size",
    1: "Bid Price",
    2: "Ask Price",
    3: "Ask Size",
    4: "Last Price",
    5: "Last Size",
    6: "High",
    7: "Low",
    8: "Volume",
    9: "Close Price",
}

# ---- IB TICK SCHEMES ---- #

# Rule 26/32 (Equity) - PriceIncrement(lowEdge=0.0, increment=0.01)
IB_1C_TICK_SCHEME_KW = dict(
    price_precision=2,
    min_tick=Price(0, 2),
    max_tick=Price(PRICE_MAX, 2),
)
IB_R26_TICK_SCHEME = FixedTickScheme(name="IB_R26_TICK_SCHEME", **IB_1C_TICK_SCHEME_KW)
register_tick_scheme(IB_R26_TICK_SCHEME)
IB_R32_TICK_SCHEME = FixedTickScheme(name="IB_R32_TICK_SCHEME", **IB_1C_TICK_SCHEME_KW)
register_tick_scheme(IB_R32_TICK_SCHEME)

# Rule 239 (Forex) - PriceIncrement(lowEdge=0.0, increment=5e-05)
IB_R239_TICK_SCHEME = FixedTickScheme(
    name="IB_R239_TICK_SCHEME",
    price_precision=5,
    min_tick=Price(0, 5),
    max_tick=Price(PRICE_MAX, 5),
    increment=5e-5,
)
register_tick_scheme(IB_R239_TICK_SCHEME)
